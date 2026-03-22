# Use Case: User Authentication Flows

> A comprehensive guide to implementing authentication in your application using Zerobase's built-in auth system: password login, OAuth2, OTP, MFA, passkeys, email verification, and password reset.

---

## Overview

Zerobase provides a full authentication stack out of the box. This guide covers every supported auth method with working examples:

1. Email/Password authentication
2. OAuth2 social login (Google, GitHub)
3. OTP (One-Time Password via email)
4. MFA (Multi-Factor Authentication with TOTP)
5. Passkeys (WebAuthn/FIDO2)
6. Email verification
7. Password reset
8. Email change
9. Token management and refresh

---

## Prerequisites

- Zerobase server running at `http://localhost:8090`
- Superuser account created
- For OAuth2: Google/GitHub OAuth app credentials
- For OTP/verification/reset: SMTP configured in settings

---

## Step 1: Create an Auth Collection

All auth features work with **auth-type** collections. Let's create a `users` collection.

```bash
TOKEN=$(curl -s -X POST http://localhost:8090/_/api/admins/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "admin@example.com", "password": "admin123456"}' \
  | jq -r '.token')

curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "users",
    "type": "auth",
    "fields": [
      {
        "name": "name",
        "type": "text",
        "required": true
      },
      {
        "name": "avatar",
        "type": "file",
        "options": {
          "maxSelect": 1,
          "maxSize": 2097152,
          "mimeTypes": ["image/jpeg", "image/png", "image/webp"]
        }
      }
    ],
    "listRule": "@request.auth.id != \"\"",
    "viewRule": "@request.auth.id != \"\"",
    "createRule": "",
    "updateRule": "@request.auth.id = id",
    "deleteRule": "@request.auth.id = id"
  }'
```

---

## Flow 1: Email/Password Authentication

The most common auth flow. Users register with email and password, then log in to receive a JWT.

### Register a new user

```bash
curl -X POST http://localhost:8090/api/collections/users/records \
  -H "Content-Type: application/json" \
  -d '{
    "email": "user@example.com",
    "password": "mySecurePassword123",
    "passwordConfirm": "mySecurePassword123",
    "name": "John Doe"
  }'
```

**Response** `200 OK`:
```json
{
  "id": "usr_abc123",
  "collectionId": "col_xyz",
  "collectionName": "users",
  "email": "user@example.com",
  "name": "John Doe",
  "verified": false,
  "created": "2026-03-21T10:00:00Z",
  "updated": "2026-03-21T10:00:00Z"
}
```

### Log in

```bash
curl -X POST http://localhost:8090/api/collections/users/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{
    "identity": "user@example.com",
    "password": "mySecurePassword123"
  }'
```

**Response** `200 OK`:
```json
{
  "token": "eyJhbGciOiJIUzI1NiIs...",
  "record": {
    "id": "usr_abc123",
    "email": "user@example.com",
    "name": "John Doe",
    "verified": false,
    "created": "2026-03-21T10:00:00Z",
    "updated": "2026-03-21T10:00:00Z"
  }
}
```

### Use the token

Include the token in the `Authorization` header for all authenticated requests:

```bash
curl -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIs..." \
  http://localhost:8090/api/collections/users/records
```

### Refresh the token

Tokens expire. Refresh before expiry:

```bash
curl -X POST http://localhost:8090/api/collections/users/auth-refresh \
  -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIs..."
```

**Response:** Same as login — returns a new `token` and `record`.

### Logout

Zerobase uses stateless JWTs. To log out, simply discard the token on the client:

```javascript
localStorage.removeItem('auth_token');
```

---

## Flow 2: OAuth2 Social Login

### Configure OAuth2 providers (superuser)

First, enable OAuth2 providers in settings:

```bash
curl -X PATCH http://localhost:8090/api/settings \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "googleAuth": {
      "enabled": true,
      "clientId": "YOUR_GOOGLE_CLIENT_ID",
      "clientSecret": "YOUR_GOOGLE_CLIENT_SECRET"
    },
    "githubAuth": {
      "enabled": true,
      "clientId": "YOUR_GITHUB_CLIENT_ID",
      "clientSecret": "YOUR_GITHUB_CLIENT_SECRET"
    }
  }'
```

### Check available auth methods

```bash
curl http://localhost:8090/api/collections/users/auth-methods
```

**Response:**
```json
{
  "password": { "enabled": true },
  "oauth2": {
    "enabled": true,
    "providers": [
      { "name": "google", "displayName": "Google", "state": "...", "authUrl": "https://accounts.google.com/o/oauth2/v2/auth?..." },
      { "name": "github", "displayName": "GitHub", "state": "...", "authUrl": "https://github.com/login/oauth/authorize?..." }
    ]
  },
  "otp": { "enabled": false },
  "mfa": { "enabled": false },
  "passkey": { "enabled": false }
}
```

### OAuth2 flow (frontend)

```javascript
// Step 1: Get available providers
const methods = await fetch(`${API}/api/collections/users/auth-methods`).then(r => r.json());
const google = methods.oauth2.providers.find(p => p.name === 'google');

// Step 2: Redirect user to the provider's auth URL
window.location.href = google.authUrl;

// Step 3: Handle callback — exchange code for token
// The provider redirects back with `code` and `state` query params
const urlParams = new URLSearchParams(window.location.search);

const authResult = await fetch(`${API}/api/collections/users/auth-with-oauth2`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    provider: 'google',
    code: urlParams.get('code'),
    state: urlParams.get('state'),
    redirectUrl: window.location.origin + '/auth/callback',
  }),
}).then(r => r.json());

// authResult = { token: "...", record: { ... }, meta: { ... } }
localStorage.setItem('auth_token', authResult.token);
```

### Manage linked OAuth identities

```bash
# List linked providers for a user
curl http://localhost:8090/api/collections/users/records/USER_ID/external-auths \
  -H "Authorization: Bearer $USER_TOKEN"

# Unlink a provider
curl -X DELETE http://localhost:8090/api/collections/users/records/USER_ID/external-auths/PROVIDER_ID \
  -H "Authorization: Bearer $USER_TOKEN"
```

---

## Flow 3: OTP (One-Time Password)

Passwordless login via email-delivered codes.

### Request an OTP

```bash
curl -X POST http://localhost:8090/api/collections/users/request-otp \
  -H "Content-Type: application/json" \
  -d '{"email": "user@example.com"}'
```

**Response:**
```json
{
  "otpId": "otp_abc123"
}
```

The user receives an email with a numeric code (e.g., `847291`).

### Authenticate with OTP

```bash
curl -X POST http://localhost:8090/api/collections/users/auth-with-otp \
  -H "Content-Type: application/json" \
  -d '{
    "otpId": "otp_abc123",
    "password": "847291"
  }'
```

**Response:** Same as password login — `{ token, record }`.

### Frontend implementation

```javascript
// Step 1: Request OTP
const { otpId } = await fetch(`${API}/api/collections/users/request-otp`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ email }),
}).then(r => r.json());

// Step 2: Show code input to user, then verify
const result = await fetch(`${API}/api/collections/users/auth-with-otp`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ otpId, password: userEnteredCode }),
}).then(r => r.json());
```

---

## Flow 4: MFA (Multi-Factor Authentication)

Add a second factor (TOTP) to password authentication.

### Step 1: Set up MFA (one-time)

After the user logs in with password, they can enable MFA:

```bash
# Request MFA setup — returns a QR code URI
curl -X POST http://localhost:8090/api/collections/users/request-mfa-setup \
  -H "Authorization: Bearer $USER_TOKEN"
```

**Response:**
```json
{
  "mfaId": "mfa_setup_123",
  "qrCodeUri": "otpauth://totp/Zerobase:user@example.com?secret=JBSWY3DPEHPK3PXP&issuer=Zerobase",
  "secret": "JBSWY3DPEHPK3PXP"
}
```

Display the QR code for the user to scan with an authenticator app (Google Authenticator, Authy, etc.).

### Step 2: Confirm MFA setup

User enters the 6-digit code from their authenticator:

```bash
curl -X POST http://localhost:8090/api/collections/users/confirm-mfa \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $USER_TOKEN" \
  -d '{
    "mfaId": "mfa_setup_123",
    "code": "123456"
  }'
```

### Step 3: Login with MFA enabled

When MFA is enabled, password login returns a **partial token** instead of a full auth token:

```bash
# Step 1: Password login
curl -X POST http://localhost:8090/api/collections/users/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "user@example.com", "password": "mySecurePassword123"}'
```

**Response (MFA required):**
```json
{
  "mfaId": "mfa_challenge_456",
  "token": null
}
```

```bash
# Step 2: Complete with TOTP code
curl -X POST http://localhost:8090/api/collections/users/auth-with-mfa \
  -H "Content-Type: application/json" \
  -d '{
    "mfaId": "mfa_challenge_456",
    "code": "654321"
  }'
```

**Response:** Full auth token `{ token, record }`.

### Frontend MFA flow

```javascript
async function loginWithMfa(email, password) {
  // Step 1: Try password login
  const step1 = await fetch(`${API}/api/collections/users/auth-with-password`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ identity: email, password }),
  }).then(r => r.json());

  // If MFA not required, we're done
  if (step1.token) {
    return step1;
  }

  // Step 2: Prompt user for TOTP code
  const totpCode = await promptUserForCode(); // Your UI prompt

  const step2 = await fetch(`${API}/api/collections/users/auth-with-mfa`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ mfaId: step1.mfaId, code: totpCode }),
  }).then(r => r.json());

  return step2;
}
```

---

## Flow 5: Passkeys (WebAuthn/FIDO2)

Hardware-key or biometric authentication using the WebAuthn standard.

### Register a passkey

```bash
# Step 1: Request registration challenge
curl -X POST http://localhost:8090/api/collections/users/request-passkey-register \
  -H "Authorization: Bearer $USER_TOKEN"
```

**Response:** WebAuthn `PublicKeyCredentialCreationOptions` (passed to browser API).

```javascript
// Frontend: Use the Web Authentication API
const options = await fetch(`${API}/api/collections/users/request-passkey-register`, {
  method: 'POST',
  headers: { Authorization: `Bearer ${token}` },
}).then(r => r.json());

// Create credential using browser API
const credential = await navigator.credentials.create({
  publicKey: options.publicKey,
});

// Send credential back to server
await fetch(`${API}/api/collections/users/confirm-passkey-register`, {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
    Authorization: `Bearer ${token}`,
  },
  body: JSON.stringify({
    id: credential.id,
    rawId: btoa(String.fromCharCode(...new Uint8Array(credential.rawId))),
    response: {
      clientDataJSON: btoa(String.fromCharCode(...new Uint8Array(credential.response.clientDataJSON))),
      attestationObject: btoa(String.fromCharCode(...new Uint8Array(credential.response.attestationObject))),
    },
    type: credential.type,
  }),
});
```

### Authenticate with a passkey

```javascript
// Step 1: Begin authentication
const beginRes = await fetch(`${API}/api/collections/users/auth-with-passkey-begin`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ email: 'user@example.com' }),
}).then(r => r.json());

// Step 2: Use browser API to get assertion
const assertion = await navigator.credentials.get({
  publicKey: beginRes.publicKey,
});

// Step 3: Complete authentication
const result = await fetch(`${API}/api/collections/users/auth-with-passkey-finish`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    challengeId: beginRes.challengeId,
    id: assertion.id,
    rawId: btoa(String.fromCharCode(...new Uint8Array(assertion.rawId))),
    response: {
      clientDataJSON: btoa(String.fromCharCode(...new Uint8Array(assertion.response.clientDataJSON))),
      authenticatorData: btoa(String.fromCharCode(...new Uint8Array(assertion.response.authenticatorData))),
      signature: btoa(String.fromCharCode(...new Uint8Array(assertion.response.signature))),
    },
    type: assertion.type,
  }),
}).then(r => r.json());

// result = { token, record }
```

---

## Flow 6: Email Verification

### Request verification email

```bash
curl -X POST http://localhost:8090/api/collections/users/request-verification \
  -H "Content-Type: application/json" \
  -d '{"email": "user@example.com"}'
```

The user receives an email with a verification link containing a token.

### Confirm verification

```bash
curl -X POST http://localhost:8090/api/collections/users/confirm-verification \
  -H "Content-Type: application/json" \
  -d '{"token": "VERIFICATION_TOKEN_FROM_EMAIL"}'
```

After confirmation, `record.verified` becomes `true`.

### Requiring verified email in rules

Use the `verified` field in API rules:

```json
{
  "createRule": "@request.auth.verified = true"
}
```

---

## Flow 7: Password Reset

### Request password reset

```bash
curl -X POST http://localhost:8090/api/collections/users/request-password-reset \
  -H "Content-Type: application/json" \
  -d '{"email": "user@example.com"}'
```

### Confirm and set new password

```bash
curl -X POST http://localhost:8090/api/collections/users/confirm-password-reset \
  -H "Content-Type: application/json" \
  -d '{
    "token": "RESET_TOKEN_FROM_EMAIL",
    "password": "newSecurePassword456",
    "passwordConfirm": "newSecurePassword456"
  }'
```

**Important:** Password reset invalidates all existing tokens for the user.

---

## Flow 8: Email Change

### Request email change

```bash
curl -X POST http://localhost:8090/api/collections/users/request-email-change \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $USER_TOKEN" \
  -d '{"newEmail": "newemail@example.com"}'
```

### Confirm email change

```bash
curl -X POST http://localhost:8090/api/collections/users/confirm-email-change \
  -H "Content-Type: application/json" \
  -d '{"token": "EMAIL_CHANGE_TOKEN_FROM_EMAIL", "password": "mySecurePassword123"}'
```

---

## Complete Auth Manager (JavaScript)

```javascript
class AuthManager {
  constructor(apiBase) {
    this.api = apiBase;
    this.token = localStorage.getItem('zb_token');
    this.user = JSON.parse(localStorage.getItem('zb_user') || 'null');
  }

  get isAuthenticated() {
    return !!this.token;
  }

  get headers() {
    const h = { 'Content-Type': 'application/json' };
    if (this.token) h['Authorization'] = `Bearer ${this.token}`;
    return h;
  }

  _save(data) {
    this.token = data.token;
    this.user = data.record;
    localStorage.setItem('zb_token', data.token);
    localStorage.setItem('zb_user', JSON.stringify(data.record));
  }

  // Password auth
  async register(email, password, name) {
    return fetch(`${this.api}/api/collections/users/records`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email, password, passwordConfirm: password, name }),
    }).then(r => r.json());
  }

  async login(email, password) {
    const res = await fetch(`${this.api}/api/collections/users/auth-with-password`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ identity: email, password }),
    }).then(r => r.json());

    if (res.mfaId) return { mfaRequired: true, mfaId: res.mfaId };
    this._save(res);
    return res;
  }

  async completeMfa(mfaId, code) {
    const res = await fetch(`${this.api}/api/collections/users/auth-with-mfa`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ mfaId, code }),
    }).then(r => r.json());
    this._save(res);
    return res;
  }

  // OTP
  async requestOtp(email) {
    return fetch(`${this.api}/api/collections/users/request-otp`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email }),
    }).then(r => r.json());
  }

  async loginWithOtp(otpId, code) {
    const res = await fetch(`${this.api}/api/collections/users/auth-with-otp`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ otpId, password: code }),
    }).then(r => r.json());
    this._save(res);
    return res;
  }

  // Token refresh
  async refresh() {
    const res = await fetch(`${this.api}/api/collections/users/auth-refresh`, {
      method: 'POST',
      headers: this.headers,
    }).then(r => r.json());
    this._save(res);
    return res;
  }

  // Logout
  logout() {
    this.token = null;
    this.user = null;
    localStorage.removeItem('zb_token');
    localStorage.removeItem('zb_user');
  }

  // Verification
  async requestVerification() {
    return fetch(`${this.api}/api/collections/users/request-verification`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: this.user.email }),
    });
  }

  // Password reset
  async requestPasswordReset(email) {
    return fetch(`${this.api}/api/collections/users/request-password-reset`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email }),
    });
  }

  // Auto-refresh token before expiry
  startAutoRefresh(intervalMs = 600000) {
    this._refreshInterval = setInterval(() => {
      if (this.isAuthenticated) this.refresh();
    }, intervalMs);
  }

  stopAutoRefresh() {
    clearInterval(this._refreshInterval);
  }
}

// Usage
const auth = new AuthManager('http://localhost:8090');

// Register and login
await auth.register('user@example.com', 'password123', 'John');
const result = await auth.login('user@example.com', 'password123');

if (result.mfaRequired) {
  const code = prompt('Enter your 2FA code:');
  await auth.completeMfa(result.mfaId, code);
}

// Use in API calls
const posts = await fetch('http://localhost:8090/api/collections/posts/records', {
  headers: auth.headers,
}).then(r => r.json());
```

---

## Summary

| Auth Method | Use Case | Endpoints |
|---|---|---|
| Password | Standard registration/login | `auth-with-password` |
| OAuth2 | Social login (Google, GitHub) | `auth-methods`, `auth-with-oauth2` |
| OTP | Passwordless email login | `request-otp`, `auth-with-otp` |
| MFA | Two-factor security | `request-mfa-setup`, `confirm-mfa`, `auth-with-mfa` |
| Passkeys | Biometric/hardware key | `request-passkey-register`, `auth-with-passkey-*` |
| Verification | Confirm email ownership | `request-verification`, `confirm-verification` |
| Password Reset | Self-service recovery | `request-password-reset`, `confirm-password-reset` |
| Email Change | Update email address | `request-email-change`, `confirm-email-change` |
| Token Refresh | Extend session | `auth-refresh` |
