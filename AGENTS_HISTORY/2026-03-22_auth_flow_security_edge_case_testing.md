# Auth Flow Security and Edge Case Testing

**Date**: 2026-03-22
**Task ID**: v6x6ozco3369frh
**Phase**: 13

## Objective

Test the full authentication system for security edge cases across all auth methods (password, OTP, OAuth2, MFA, passkeys).

## Files Created/Modified

### New Files
- `crates/zerobase-auth/src/security_tests.rs` — 21 unit tests covering security areas 1–6
- `AGENTS_HISTORY/2026-03-22_auth_flow_security_edge_case_testing.md` — this log

### Modified Files
- `crates/zerobase-auth/src/lib.rs` — registered `security_tests` module (cfg(test))
- `crates/zerobase-api/src/middleware/rate_limit.rs` — added 5 security-specific rate limiting tests

## Security Checks Implemented

### 1. Token Invalidation (5 tests)
- `password_change_rotates_token_key_invalidating_all_tokens` — verifies auth, refresh, and file tokens all invalidated when tokenKey rotates
- `token_key_change_does_not_affect_other_users` — per-user isolation
- `mfa_partial_token_invalidated_by_key_change` — MfaPartial tokens invalidated by key rotation
- `password_reset_token_invalidated_by_key_change` — PasswordReset tokens invalidated
- `cross_type_token_reuse_blocked_even_with_valid_key` — auth token cannot be used as refresh/MfaPartial/PasswordReset

### 2. OTP Brute-Force Protection (4 tests)
- `otp_exactly_max_attempts_exhausts_then_rejects_correct_code` — 5 wrong attempts then correct code fails
- `otp_attempt_4_wrong_then_correct_succeeds` — 4 wrong attempts still allows correct code (boundary test)
- `otp_code_single_use_prevents_replay` — code cannot be reused after successful verification
- `otp_for_unknown_email_returns_otp_id_without_leaking` — anti-enumeration: returns valid otp_id, no email sent

### 3. OAuth2 State Parameter CSRF Protection (2 tests)
- `oauth2_state_values_are_unique_per_request` — 100 state values all unique
- `oauth2_state_has_sufficient_entropy` — state token at least 15 characters

### 4. MFA Bypass Prevention (4 tests)
- `mfa_partial_token_rejected_as_auth_token` — MfaPartial cannot access Auth-protected resources
- `mfa_partial_token_only_accepted_as_mfa_partial` — rejected for all 6 other token types
- `auth_token_cannot_be_used_as_mfa_partial` — Auth token cannot bypass MFA step
- `mfa_partial_token_has_short_lifetime` — verified 5-minute (300s) expiry

### 5. Race Conditions (2 tests)
- `concurrent_otp_verifications_only_one_succeeds` — 10 threads racing, exactly 1 succeeds (Mutex-based store)
- `concurrent_otp_requests_for_same_email_all_produce_valid_ids` — 10 concurrent OTP requests all produce unique IDs

### 6. Timing Attacks (4 tests)
- `password_verification_timing_consistent_correct_vs_incorrect` — Argon2id timing ratio within [0.2, 5.0]
- `password_verification_timing_consistent_short_vs_long_wrong_password` — short vs long password timing ratio within [0.1, 10.0]
- `token_validation_timing_consistent_valid_vs_invalid_key` — JWT key validation timing ratio within [0.1, 10.0]
- `argon2id_uses_owasp_recommended_parameters` — verifies m=19456, t=2, p=1 in PHC string

### 7. Rate Limiting on Auth Endpoints (5 tests in zerobase-api)
- `security_login_endpoint_enforces_auth_rate_limit` — auth-with-password rate limited with Retry-After header
- `security_otp_and_login_share_auth_bucket` — OTP and login endpoints share the Auth rate limit bucket
- `security_password_reset_shares_auth_bucket` — password reset endpoint uses Auth bucket
- `security_auth_rate_limit_per_ip_isolation` — different IPs tracked independently
- `security_exhausted_auth_bucket_does_not_block_default_routes` — Auth exhaustion doesn't affect Default routes

## Test Results

All 27 tests pass:
- 21 in `zerobase-auth::security_tests`
- 6 in `zerobase-api::middleware::rate_limit` (5 new + 1 existing security_headers test)

## Architecture Notes

- Token invalidation relies on `tokenKey` claim in JWT — changing the stored key invalidates all existing tokens for that user
- OTP store uses `Mutex<HashMap>` ensuring single-threaded access for race condition safety
- MFA bypass prevention is enforced at the JWT type level — `TokenType::MfaPartial` cannot validate as any other type
- Rate limiting uses per-(IP, RouteCategory) token buckets via DashMap
- Password hashing uses Argon2id with OWASP-recommended parameters (m=19456, t=2, p=1)
