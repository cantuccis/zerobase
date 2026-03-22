# Zerobase Admin — Navigation Flow Diagram

## Primary Navigation Graph

```
                         ┌──────────────┐
                         │  First Visit │
                         └──────┬───────┘
                                │
                     ┌──────────▼──────────┐
                     │  Superuser exists?   │
                     └──┬──────────────┬───┘
                   No   │              │  Yes
                        ▼              ▼
              ┌─────────────┐  ┌───────────────┐
              │ /_/auth/     │  │ /_/auth/login │
              │ setup        │  │               │
              │              │  │ email         │
              │ Create       │  │ password      │
              │ superuser    │  │ [Sign In]     │
              └──────┬──────┘  └───┬───────────┘
                     │             │
                     │  auto-login │  on success
                     └──────┬──────┘
                            ▼
┌───────────────────────────────────────────────────────────┐
│                                                           │
│   ┌─────────────────────────────────────────────────┐     │
│   │              AUTHENTICATED SHELL                │     │
│   │                                                 │     │
│   │   Topbar: [Logo] [⌘K Search] [Theme] [User]    │     │
│   │   ┌────────┬────────────────────────────────┐   │     │
│   │   │Sidebar │  Content Area                  │   │     │
│   │   │        │                                │   │     │
│   │   │        │                                │   │     │
│   │   └────────┴────────────────────────────────┘   │     │
│   │   Status: [version] [db] [uptime]               │     │
│   └─────────────────────────────────────────────────┘     │
│                                                           │
│   Sidebar routes:                                         │
│                                                           │
│   ┌────────────┐  ┌──────────────┐  ┌────────────────┐   │
│   │ /_/        │  │/_/collections│  │/_/settings     │   │
│   │ Dashboard  │  │ Collections  │  │ Settings       │   │
│   │            │  │              │  │ (5 tabs)       │   │
│   │ Stats      │  │ List + Edit  │  │                │   │
│   │ Charts     │  │ Schema       │  │ app│mail│stor  │   │
│   │ Activity   │  │              │  │ auth│backups   │   │
│   └────────────┘  └──────┬───────┘  └────────────────┘   │
│                          │                                │
│                          ▼                                │
│                   ┌──────────────┐                        │
│                   │/_/collections│   ┌──────────────┐     │
│                   │/:name/records│   │ /_/logs      │     │
│                   │              │   │ Logs Viewer  │     │
│                   │ Table view   │   │              │     │
│                   │ Filter/Sort  │───│ Filter/Date  │     │
│                   │ Pagination   │   │ Stats bar    │     │
│                   └──────┬───────┘   │ Expandable   │     │
│                          │           └──────────────┘     │
│                          ▼                                │
│                   ┌──────────────┐   ┌──────────────┐     │
│                   │ Record Editor│   │ /_/backups   │     │
│                   │ (Drawer)     │   │              │     │
│                   │              │   │ Auto config  │     │
│                   │ Auto-form    │   │ Backup list  │     │
│                   │ from schema  │   │ Restore/DL   │     │
│                   └──────────────┘   └──────────────┘     │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

## User Journey: Collection CRUD Lifecycle

```
1. CREATE COLLECTION
   /_/collections → [+ New Collection] → Modal opens
   → Enter name, select type (base/auth/view)
   → [Create] → Collection appears in left panel

2. DEFINE SCHEMA
   /_/collections → Select collection → Right panel shows fields
   → [+ New Field] → Field editor opens
   → Select type → Configure options → [Save Field]
   → Repeat for all fields
   → [Save Changes] → Schema persisted

3. SET ACCESS RULES
   /_/collections → Select collection → Scroll to API Rules
   → Edit list/view/create/update/delete rules
   → [Save Changes] → Rules persisted

4. MANAGE RECORDS
   /_/collections → Select collection → [View Records]
   → /_/collections/:name/records → Browse table
   → [+ New Record] → Drawer opens → Fill form → [Save]
   → Click row → Drawer opens → Edit → [Save]
   → Select rows → [Delete Selected] → Confirm → Deleted

5. MONITOR
   /_/logs → Filter by collection API path
   /_/ → Dashboard shows collection stats
```

## User Journey: Settings Configuration

```
1. APPLICATION METADATA
   /_/settings?tab=app → Set app name, URL, sender info → [Save]

2. CONFIGURE EMAIL
   /_/settings?tab=mail → Enable SMTP → Enter host/port/creds
   → [Send Test Email] → Verify → [Save]

3. CONFIGURE STORAGE
   /_/settings?tab=storage → Select Local or S3
   → If S3: enter bucket/region/endpoint/keys
   → [Test Connection] → Verify → [Save]

4. CONFIGURE AUTH
   /_/settings?tab=auth → Toggle password/OTP/MFA/passkeys
   → Configure OAuth: [Configure ▸] on provider
   → Enter Client ID + Secret → [Save]

5. CONFIGURE BACKUPS
   /_/settings?tab=backups → Enable auto-backup
   → Set interval + max retention
   → [Create Backup Now] for immediate backup
   → [Save]
```

## Global Interactions Available From Any Page

| Trigger | Action | Target |
|---------|--------|--------|
| `⌘K` / `Ctrl+K` | Open command palette | Search collections, records, settings |
| Sidebar click | Navigate to section | Corresponding route |
| Theme toggle | Switch light/dark | Applies globally |
| User menu → Sign Out | End session | Redirect to /_/auth/login |
| User menu → Change Password | Open modal | Password change form |
| Toast click | Dismiss or navigate | Context-dependent |

## Auth Guard Logic

```
Every route under /_/ (except /_/auth/*):
  1. Check localStorage for auth token
  2. Validate token expiry client-side
  3. If invalid → redirect to /_/auth/login?redirect={current_path}
  4. On login success → redirect to stored path or /_/

Token refresh:
  - On every API call, check response for 401
  - If 401 → clear token → redirect to login
  - Token stored in httpOnly cookie (preferred) or localStorage
```
