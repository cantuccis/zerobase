# Zerobase Admin Dashboard — UI/UX Design Document

**Version:** 1.0
**Date:** 2026-03-21
**Stack:** AstroJS 5 + Tailwind CSS 4 + TypeScript
**Served at:** `/_/`

---

## 1. Design Direction

### Aesthetic: Refined Industrial

A professional, developer-oriented admin panel that feels like a precision tool. Inspired by PocketBase's functional clarity but elevated with modern craft.

- **Dark-first** theme (with light mode toggle)
- **Monochrome palette** with a single accent color (electric blue)
- **Dense information display** — respects the power-user who wants to see data, not chrome
- **Typography:** JetBrains Mono for code/IDs, General Sans for UI text
- **Micro-interactions** on state changes (row expand, toast, field add/remove)

### Color System (CSS Variables)

```
--zb-bg-primary:     hsl(220 20% 6%);       /* #0d0f14 — deep charcoal */
--zb-bg-secondary:   hsl(220 18% 10%);      /* #151820 — card/panel bg */
--zb-bg-tertiary:    hsl(220 16% 14%);      /* #1e2230 — input bg / hover */
--zb-bg-elevated:    hsl(220 14% 18%);      /* #282d3a — dropdown/modal */

--zb-border:         hsl(220 14% 20%);      /* subtle dividers */
--zb-border-focus:   hsl(215 85% 55%);      /* electric blue focus ring */

--zb-text-primary:   hsl(0 0% 95%);         /* #f2f2f2 */
--zb-text-secondary: hsl(220 10% 60%);      /* #8b92a5 — muted labels */
--zb-text-tertiary:  hsl(220 8% 42%);       /* #62687a — disabled/hint */

--zb-accent:         hsl(215 85% 55%);      /* #3b82f6 — primary action */
--zb-accent-hover:   hsl(215 85% 48%);      /* hover state */
--zb-success:        hsl(142 60% 45%);      /* #2eb868 */
--zb-warning:        hsl(38 92% 55%);       /* #e8a020 */
--zb-danger:         hsl(0 72% 55%);        /* #d94040 */

/* Light mode overrides applied via .light class on <html> */
--zb-bg-primary:     hsl(0 0% 98%);
--zb-bg-secondary:   hsl(0 0% 100%);
--zb-bg-tertiary:    hsl(220 14% 96%);
--zb-text-primary:   hsl(220 20% 10%);
--zb-text-secondary: hsl(220 10% 45%);
--zb-border:         hsl(220 14% 88%);
```

### Spacing Scale

```
--space-1: 4px;   --space-2: 8px;   --space-3: 12px;
--space-4: 16px;  --space-5: 20px;  --space-6: 24px;
--space-8: 32px;  --space-10: 40px; --space-12: 48px;
```

### Border Radius

```
--radius-sm: 4px;  --radius-md: 6px;  --radius-lg: 8px;  --radius-xl: 12px;
```

---

## 2. Application Shell & Navigation

### Layout Structure

```
┌─────────────────────────────────────────────────────────────────┐
│ TOPBAR  [ZB logo + "Zerobase"]         [search ⌘K]  [◑] [👤]  │
├──────────┬──────────────────────────────────────────────────────┤
│          │                                                      │
│ SIDEBAR  │  MAIN CONTENT AREA                                   │
│          │                                                      │
│ ≡ Dash   │  ┌──────────────────────────────────────────────┐    │
│ ▤ Colls  │  │  Page header (breadcrumb + title + actions)  │    │
│ 🔐 Auth  │  ├──────────────────────────────────────────────┤    │
│ ⚙ Set.   │  │                                              │    │
│ 📋 Logs  │  │  Page body (scrollable)                      │    │
│ 💾 Back  │  │                                              │    │
│          │  │                                              │    │
│          │  └──────────────────────────────────────────────┘    │
│          │                                                      │
├──────────┴──────────────────────────────────────────────────────┤
│ STATUS BAR  [v0.1.0]  [sqlite: pb_data.db]  [uptime: 2h 14m]  │
└─────────────────────────────────────────────────────────────────┘
```

### Sidebar Navigation Items

| Icon | Label | Route | Description |
|------|-------|-------|-------------|
| Grid | Dashboard | `/_/` | Overview stats |
| Table | Collections | `/_/collections` | Schema manager |
| Shield | Auth Providers | `/_/auth` | Auth config |
| Gear | Settings | `/_/settings` | App settings |
| List | Logs | `/_/logs` | Request logs |
| Archive | Backups | `/_/backups` | DB backups |

- Sidebar is **collapsible** to icon-only mode (64px → 48px)
- Active route gets accent-colored left border + tinted background
- Sidebar bottom: superuser avatar + email + sign out

### Topbar

- **Left:** Zerobase logo (geometric "Z" mark) + wordmark
- **Center:** Global search (`⌘K` / `Ctrl+K`) — searches collections, records, settings
- **Right:** Theme toggle (sun/moon), superuser menu (avatar dropdown)

### Responsive Behavior

- **≥1280px:** Full sidebar + content
- **768–1279px:** Collapsed sidebar (icons only), content fills
- **<768px:** Sidebar becomes slide-out drawer (hamburger toggle in topbar)

---

## 3. View Wireframes

### 3.1 Login Page (`/_/auth/login`)

```
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│                                                             │
│                    ┌────────────────────┐                   │
│                    │                    │                   │
│                    │    [Z] ZEROBASE    │                   │
│                    │                    │                   │
│                    │  ── Admin Login ── │                   │
│                    │                    │                   │
│                    │  Email             │                   │
│                    │  ┌──────────────┐  │                   │
│                    │  │              │  │                   │
│                    │  └──────────────┘  │                   │
│                    │                    │                   │
│                    │  Password          │                   │
│                    │  ┌──────────────┐  │                   │
│                    │  │          [👁] │  │                   │
│                    │  └──────────────┘  │                   │
│                    │                    │                   │
│                    │  [ Sign In ~~~~~~] │                   │
│                    │                    │                   │
│                    │  Forgot password?  │                   │
│                    │                    │                   │
│                    └────────────────────┘                   │
│                                                             │
│                    Powered by Zerobase v0.1.0               │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**Behavior:**
- Full-bleed dark background with subtle noise texture
- Card is centered vertically and horizontally (max-width: 400px)
- Animated gradient border on focus
- Error messages appear inline below each field
- On first run (no superuser), shows "Create Superuser Account" form instead
- After login, redirect to `/_/`

---

### 3.2 Dashboard Overview (`/_/`)

```
┌─ MAIN CONTENT ──────────────────────────────────────────────┐
│                                                              │
│  Dashboard                                                   │
│                                                              │
│  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌──────────┐ │
│  │ COLLECTIONS │ │   RECORDS  │ │ REQUESTS/h │ │ DB SIZE  │ │
│  │     12      │ │   4,281    │ │    842     │ │  24 MB   │ │
│  │  +2 today   │ │ +126 today │ │  ↑12%      │ │          │ │
│  └────────────┘ └────────────┘ └────────────┘ └──────────┘ │
│                                                              │
│  ┌──────────────────────────────┐ ┌────────────────────────┐│
│  │ Requests (24h)               │ │ Recent Activity        ││
│  │                              │ │                        ││
│  │  ▁▃▅▇█▇▅▃▁▂▄▆█▇▅▃▂▁▃▅▇█▇▅ │ │ • users: +3 records   ││
│  │  ───────────────────────────  │ │ • posts: schema edit   ││
│  │  00:00          12:00  now   │ │ • settings: SMTP upd.  ││
│  │                              │ │ • backup: auto 02:00   ││
│  └──────────────────────────────┘ │ • auth: new OAuth cfg  ││
│                                   │                        ││
│  ┌──────────────────────────────┐ │                        ││
│  │ Collections Overview         │ └────────────────────────┘│
│  │ ┌─────────┬──────┬────────┐  │                           │
│  │ │ Name    │ Type │ Records│  │                           │
│  │ ├─────────┼──────┼────────┤  │                           │
│  │ │ users   │ auth │  1,204 │  │                           │
│  │ │ posts   │ base │  2,847 │  │                           │
│  │ │ tags    │ base │    230 │  │                           │
│  │ │ ...     │      │        │  │                           │
│  │ └─────────┴──────┴────────┘  │                           │
│  └──────────────────────────────┘                           │
└──────────────────────────────────────────────────────────────┘
```

**Stat Cards:**
- 4 KPI cards in a responsive grid (4 cols → 2 cols → 1 col)
- Each card: metric label, large number, trend indicator (optional)
- Subtle hover elevation

**Charts:**
- Request volume sparkline (24h, no library overhead — CSS/SVG inline)
- Collections table with clickable rows → navigate to collection

---

### 3.3 Collections Manager (`/_/collections`)

```
┌─ MAIN CONTENT ──────────────────────────────────────────────┐
│                                                              │
│  Collections                            [+ New Collection]   │
│                                                              │
│  ┌─ SIDEBAR LIST ──┐  ┌─ COLLECTION DETAIL ───────────────┐ │
│  │                  │  │                                   │ │
│  │  🔍 Filter...    │  │  users (auth)        [API] [⚙]   │ │
│  │                  │  │                                   │ │
│  │  ● users  (auth) │  │  Fields ─────────────────────────│ │
│  │    posts  (base) │  │  ┌──────────────────────────────┐│ │
│  │    tags   (base) │  │  │ ≡ id        autoId  [system] ││ │
│  │    media  (base) │  │  │ ≡ email     email   required ││ │
│  │    stats  (view) │  │  │ ≡ name      text             ││ │
│  │                  │  │  │ ≡ avatar    file             ││ │
│  │                  │  │  │ ≡ role      select           ││ │
│  │                  │  │  │ ≡ created   autodate[system] ││ │
│  │                  │  │  │ ≡ updated   autodate[system] ││ │
│  │                  │  │  │                              ││ │
│  │                  │  │  │ [+ New Field]                ││ │
│  │                  │  │  └──────────────────────────────┘│ │
│  │                  │  │                                   │ │
│  │                  │  │  API Rules ───────────────────────│ │
│  │                  │  │  List:   @request.auth.id != ""  │ │
│  │                  │  │  View:   @request.auth.id != ""  │ │
│  │                  │  │  Create: @request.auth.id != ""  │ │
│  │                  │  │  Update: @request.auth.id = id   │ │
│  │                  │  │  Delete: @request.auth.role = .. │ │
│  │                  │  │                                   │ │
│  │  [+ Collection]  │  │  [View Records]  [Save Changes]  │ │
│  └──────────────────┘  └───────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
```

**Two-Panel Layout:**
- **Left panel (240px):** Scrollable list of all collections, filterable. Collection type shown as badge (auth/base/view). Active selection highlighted.
- **Right panel:** Selected collection's schema editor.

**Field List:**
- Each field row: drag handle (≡) | name | type badge | constraint badges (required, unique, system)
- Click to expand inline editor for that field
- Drag-to-reorder fields
- System fields (id, created, updated) are dimmed and non-removable

**API Rules Section:**
- 5 rule inputs (list, view, create, update, delete)
- Syntax-highlighted text input with auto-complete for `@request.*` tokens
- Empty rule = locked (no access). `""` = public.

**Actions:**
- "View Records" → navigates to `/_/collections/{name}/records`
- "API" button → shows API preview panel (curl examples)
- Save Changes → PUT to schema API with optimistic update

---

### 3.4 Collection Schema Editor (Field Edit Modal)

```
┌─ ADD / EDIT FIELD ──────────────────────────────────────────┐
│                                                              │
│  Field Name                                                  │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ display_name                                           │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                              │
│  Field Type                                                  │
│  ┌─────────────────────────────────────────┐                 │
│  │ ▾ Text                                  │                 │
│  ├─────────────────────────────────────────┤                 │
│  │  Text       Number      Bool            │                 │
│  │  Email      URL         DateTime        │                 │
│  │  Select     MultiSelect File            │                 │
│  │  Relation   JSON        Editor          │                 │
│  │  Password   AutoDate                    │                 │
│  └─────────────────────────────────────────┘                 │
│                                                              │
│  ── Type Options ──────────────────────────────────────────  │
│                                                              │
│  Min length    ┌──────┐   Max length   ┌──────┐             │
│                │ 0    │                │ 500   │             │
│                └──────┘                └──────┘             │
│  Pattern (regex)                                             │
│  ┌────────────────────────────────────────────────────────┐  │
│  │                                                        │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                              │
│  ── Constraints ───────────────────────────────────────────  │
│                                                              │
│  [✓] Required    [ ] Unique    [ ] Searchable                │
│                                                              │
│                                 [Cancel]  [Save Field]       │
└──────────────────────────────────────────────────────────────┘
```

**Type-specific options panels:**

| FieldType | Options Shown |
|-----------|---------------|
| Text | min/max length, pattern (regex), searchable |
| Number | min, max, noDecimal |
| Bool | (none) |
| Email | exceptDomains, onlyDomains |
| URL | exceptDomains, onlyDomains |
| DateTime | min, max |
| AutoDate | onCreate, onUpdate |
| Select | values[] (tag input), maxSelect |
| MultiSelect | values[] (tag input), maxSelect |
| File | maxSelect, maxSize, mimeTypes[] |
| Relation | collectionId (dropdown), cascadeDelete |
| JSON | maxSize |
| Editor | maxSize, searchable |
| Password | min/max length, pattern |

---

### 3.5 Records Browser (`/_/collections/{name}/records`)

```
┌─ MAIN CONTENT ──────────────────────────────────────────────┐
│                                                              │
│  ← Collections / users                    [+ New Record]     │
│                                                              │
│  ┌─ TOOLBAR ───────────────────────────────────────────────┐│
│  │ 🔍 Filter: ┌────────────────────────┐  Sort: ┌───────┐ ││
│  │            │ email ~ "gmail"        │        │created││
│  │            └────────────────────────┘        └───────┘ ││
│  │ Showing 1-25 of 1,204                  Per page: [25 ▾]││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
│  ┌──────────────────────────────────────────────────────────┐│
│  │ □  id         email              name       created      ││
│  │────────────────────────────────────────────────────────── ││
│  │ □  abc12..    alice@gm..         Alice W.   2026-03-20   ││
│  │ □  def34..    bob@comp..         Bob K.     2026-03-19   ││
│  │ □  ghi56..    carol@ex..         Carol P.   2026-03-18   ││
│  │ □  jkl78..    dave@test..        Dave L.    2026-03-17   ││
│  │ □  mno90..    eve@mail..         Eve R.     2026-03-15   ││
│  │                                                          ││
│  │  ◄ 1 2 3 ... 48 ►                                       ││
│  └──────────────────────────────────────────────────────────┘│
│                                                              │
│  Selected: 0                     [Delete Selected]           │
└──────────────────────────────────────────────────────────────┘
```

**Data Table Features:**
- Column headers with click-to-sort (asc/desc/none toggle)
- Resizable columns via drag handle on header borders
- Checkbox selection for bulk operations
- Row click opens record editor drawer (slides from right)
- Pagination with page size selector (10, 25, 50, 100)
- Filter bar with PocketBase filter syntax (auto-complete for field names)
- Long text values truncated with ellipsis
- File fields show thumbnail previews
- Relation fields show linked record ID as clickable chip

---

### 3.6 Record Editor (Slide-over Drawer)

```
                              ┌─ EDIT RECORD ─────────────────┐
                              │                            [✕] │
                              │  Record: abc12de              │
                              │  Created: 2026-03-20 14:22    │
                              │  Updated: 2026-03-21 09:15    │
                              │                               │
                              │  email *                      │
                              │  ┌─────────────────────────┐  │
                              │  │ alice@gmail.com         │  │
                              │  └─────────────────────────┘  │
                              │                               │
                              │  name                         │
                              │  ┌─────────────────────────┐  │
                              │  │ Alice Wonderland        │  │
                              │  └─────────────────────────┘  │
                              │                               │
                              │  avatar                       │
                              │  ┌─────────────────────────┐  │
                              │  │ 📎 alice.jpg (42 KB)    │  │
                              │  │ [Replace] [Remove]      │  │
                              │  └─────────────────────────┘  │
                              │                               │
                              │  role                         │
                              │  ┌─────────────────────────┐  │
                              │  │ ▾ admin                 │  │
                              │  └─────────────────────────┘  │
                              │                               │
                              │  ── Relations ──              │
                              │  posts (3)  [View ↗]          │
                              │                               │
                              │                               │
                              │  [Delete]        [Save]       │
                              └───────────────────────────────┘
```

**Behavior:**
- Slides in from right (480px wide, or full-screen on mobile)
- Auto-generates form fields based on collection schema
- Field types map to appropriate input widgets (see Component Inventory)
- Required fields marked with `*`
- System fields (id, created, updated) displayed as read-only metadata
- Relation fields show linked records with navigation
- File fields support drag-and-drop upload
- Delete requires confirmation modal
- Unsaved changes warning on close attempt

---

### 3.7 Settings Pages (`/_/settings`)

Tabs within the settings page:

```
┌─ MAIN CONTENT ──────────────────────────────────────────────┐
│                                                              │
│  Settings                                                    │
│                                                              │
│  [Application] [Mail] [Storage] [Auth Providers] [Backups]   │
│  ─────────────────────────────────────────────────────────── │
│                                                              │
│  ┌─ APPLICATION TAB ───────────────────────────────────────┐ │
│  │                                                         │ │
│  │  Application Name                                       │ │
│  │  ┌───────────────────────────────────────────────────┐  │ │
│  │  │ My App                                            │  │ │
│  │  └───────────────────────────────────────────────────┘  │ │
│  │                                                         │ │
│  │  Application URL                                        │ │
│  │  ┌───────────────────────────────────────────────────┐  │ │
│  │  │ https://myapp.example.com                         │  │ │
│  │  └───────────────────────────────────────────────────┘  │ │
│  │                                                         │ │
│  │  Sender Name                                            │ │
│  │  ┌───────────────────────────────────────────────────┐  │ │
│  │  │ My App Support                                    │  │ │
│  │  └───────────────────────────────────────────────────┘  │ │
│  │                                                         │ │
│  │  Sender Address                                         │ │
│  │  ┌───────────────────────────────────────────────────┐  │ │
│  │  │ support@myapp.example.com                         │  │ │
│  │  └───────────────────────────────────────────────────┘  │ │
│  │                                                         │ │
│  │                                        [Save Changes]   │ │
│  └─────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
```

#### Settings Tab: Mail (SMTP)

```
  ┌─ MAIL TAB ─────────────────────────────────────────────┐
  │                                                         │
  │  SMTP Enabled   [●───]  (toggle)                        │
  │                                                         │
  │  SMTP Host                     Port                     │
  │  ┌─────────────────────────┐   ┌──────┐                │
  │  │ smtp.gmail.com          │   │ 587  │                │
  │  └─────────────────────────┘   └──────┘                │
  │                                                         │
  │  Username                                               │
  │  ┌───────────────────────────────────────────────────┐  │
  │  │ user@gmail.com                                    │  │
  │  └───────────────────────────────────────────────────┘  │
  │                                                         │
  │  Password                                               │
  │  ┌───────────────────────────────────────────────────┐  │
  │  │ ●●●●●●●●                                  [👁]   │  │
  │  └───────────────────────────────────────────────────┘  │
  │                                                         │
  │  [✓] Use TLS                                            │
  │                                                         │
  │  [Send Test Email]                      [Save Changes]  │
  └─────────────────────────────────────────────────────────┘
```

#### Settings Tab: Storage (S3)

```
  ┌─ STORAGE TAB ──────────────────────────────────────────┐
  │                                                         │
  │  Storage Backend                                        │
  │  (●) Local filesystem   ( ) S3-compatible               │
  │                                                         │
  │  ── S3 Configuration (disabled when Local selected) ──  │
  │                                                         │
  │  Bucket              Region                             │
  │  ┌──────────────┐   ┌──────────────┐                   │
  │  │              │   │              │                   │
  │  └──────────────┘   └──────────────┘                   │
  │                                                         │
  │  Endpoint                                               │
  │  ┌───────────────────────────────────────────────────┐  │
  │  │                                                   │  │
  │  └───────────────────────────────────────────────────┘  │
  │                                                         │
  │  Access Key            Secret Key                       │
  │  ┌──────────────┐   ┌──────────────┐                   │
  │  │              │   │ ●●●●●●  [👁] │                   │
  │  └──────────────┘   └──────────────┘                   │
  │                                                         │
  │  [✓] Force path style                                   │
  │                                                         │
  │  [Test Connection]                      [Save Changes]  │
  └─────────────────────────────────────────────────────────┘
```

#### Settings Tab: Auth Providers

```
  ┌─ AUTH PROVIDERS TAB ───────────────────────────────────┐
  │                                                         │
  │  ── Password Auth ──                                    │
  │  Enabled [───●]   Min length: [8 ▾]                    │
  │                                                         │
  │  ── OTP (Email) ──                                      │
  │  Enabled [───●]   Code length: [6 ▾]   TTL: [300s ▾]  │
  │                                                         │
  │  ── MFA ──                                              │
  │  Enabled [●───]   Methods: Password + OTP              │
  │                                                         │
  │  ── Passkeys (WebAuthn) ──                              │
  │  Enabled [●───]   RP Name: [My App        ]            │
  │                    RP ID:   [myapp.example.com]         │
  │                                                         │
  │  ── OAuth2 Providers ──────────────────────────────────│
  │                                                         │
  │  ┌──────────────────────────────────────────────────┐  │
  │  │  [G] Google          Enabled  [Configure ▸]      │  │
  │  │  [M] Microsoft       Disabled [Configure ▸]      │  │
  │  │  [GH] GitHub         Disabled [Configure ▸]      │  │
  │  │  [+] Add provider...                             │  │
  │  └──────────────────────────────────────────────────┘  │
  │                                                         │
  │                                        [Save Changes]   │
  └─────────────────────────────────────────────────────────┘
```

**OAuth Provider Config (expanded):**

```
  ┌─ Google OAuth2 Config ──────────────────────────────────┐
  │                                                          │
  │  Enabled [───●]                                          │
  │                                                          │
  │  Client ID                                               │
  │  ┌────────────────────────────────────────────────────┐  │
  │  │ 123456789.apps.googleusercontent.com               │  │
  │  └────────────────────────────────────────────────────┘  │
  │                                                          │
  │  Client Secret                                           │
  │  ┌────────────────────────────────────────────────────┐  │
  │  │ ●●●●●●●●●●●●●●●●●●                          [👁] │  │
  │  └────────────────────────────────────────────────────┘  │
  │                                                          │
  │  Redirect URL (read-only)                                │
  │  ┌────────────────────────────────────────────────────┐  │
  │  │ https://myapp.com/api/oauth2-redirect         [📋] │  │
  │  └────────────────────────────────────────────────────┘  │
  │                                                          │
  │  [Cancel]                                      [Save]    │
  └──────────────────────────────────────────────────────────┘
```

#### Settings Tab: Backups

```
  ┌─ BACKUPS TAB ──────────────────────────────────────────┐
  │                                                         │
  │  Auto Backup                                            │
  │  Enabled [───●]   Interval: [Every 24 hours ▾]         │
  │  Max backups to keep: [5 ▾]                             │
  │                                                         │
  │                              [Create Backup Now]        │
  │                                                         │
  │  ── Existing Backups ──────────────────────────────────│
  │                                                         │
  │  ┌──────────────────────────────────────────────────┐  │
  │  │  Filename               Size    Date             │  │
  │  │─────────────────────────────────────────────────│  │
  │  │  pb_backup_20260321.zip 12 MB   2026-03-21 02:00│  │
  │  │                                [↓] [Restore] [✕]│  │
  │  │  pb_backup_20260320.zip 11 MB   2026-03-20 02:00│  │
  │  │                                [↓] [Restore] [✕]│  │
  │  │  pb_backup_20260319.zip 11 MB   2026-03-19 02:00│  │
  │  │                                [↓] [Restore] [✕]│  │
  │  └──────────────────────────────────────────────────┘  │
  │                                                         │
  │                                        [Save Changes]   │
  └─────────────────────────────────────────────────────────┘
```

---

### 3.8 Logs Viewer (`/_/logs`)

```
┌─ MAIN CONTENT ──────────────────────────────────────────────┐
│                                                              │
│  Logs                                                        │
│                                                              │
│  ┌─ TOOLBAR ───────────────────────────────────────────────┐│
│  │ 🔍 Filter:  ┌──────────────────────────┐                ││
│  │             │ status >= 400            │                ││
│  │             └──────────────────────────┘                ││
│  │ Date range: [2026-03-20] → [2026-03-21]  Level: [All ▾]││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
│  ┌─ STATS BAR ─────────────────────────────────────────────┐│
│  │  Total: 4,212  │  2xx: 3,840  │  4xx: 312  │  5xx: 60  ││
│  └─────────────────────────────────────────────────────────┘│
│                                                              │
│  ┌──────────────────────────────────────────────────────────┐│
│  │ Time       Method  Path             Status  Duration     ││
│  │──────────────────────────────────────────────────────────││
│  │ 09:15:42   GET     /api/users       200     12ms        ││
│  │ 09:15:38   POST    /api/users       201     45ms        ││
│  │ 09:15:30   GET     /api/posts       200      8ms        ││
│  │ 09:14:55   DELETE  /api/posts/abc   403     3ms    ⚠    ││
│  │ 09:14:22   POST    /api/auth/login  401     156ms  ⚠    ││
│  │ 09:14:01   GET     /api/settings    500     2ms    ●    ││
│  │                                                          ││
│  │  ◄ 1 2 3 ... 169 ►                                      ││
│  └──────────────────────────────────────────────────────────┘│
│                                                              │
│  ┌─ LOG DETAIL (expanded row) ─────────────────────────────┐│
│  │ Request:  POST /api/collections/users/records            ││
│  │ Status:   201 Created                                    ││
│  │ Duration: 45ms                                           ││
│  │ IP:       192.168.1.42                                   ││
│  │ Auth:     superuser (admin@zb.io)                        ││
│  │                                                          ││
│  │ Request Body:                                            ││
│  │ ┌────────────────────────────────────────────────────┐   ││
│  │ │ { "email": "alice@example.com", "name": "Alice" } │   ││
│  │ └────────────────────────────────────────────────────┘   ││
│  │                                                          ││
│  │ Response Body:                                           ││
│  │ ┌────────────────────────────────────────────────────┐   ││
│  │ │ { "id": "abc12de", "email": "alice@example..." }  │   ││
│  │ └────────────────────────────────────────────────────┘   ││
│  └──────────────────────────────────────────────────────────┘│
└──────────────────────────────────────────────────────────────┘
```

**Features:**
- Filterable by date range, status code range, method, path pattern
- Stats summary bar (total requests, 2xx/4xx/5xx counts)
- Color-coded status: green (2xx), yellow (4xx), red (5xx)
- Click row to expand log detail (request/response bodies, headers, IP, auth info)
- Auto-refresh toggle (live tail mode)
- JSON bodies displayed with syntax highlighting

---

## 4. Component Inventory

### 4.1 Layout Components

| Component | Description | Props |
|-----------|-------------|-------|
| `AppShell` | Root layout: topbar + sidebar + content area | `children` |
| `Sidebar` | Navigation sidebar with collapse toggle | `collapsed`, `onToggle` |
| `SidebarItem` | Nav link with icon, label, active state | `icon`, `label`, `href`, `active`, `badge?` |
| `Topbar` | App header with search, theme toggle, user menu | `user` |
| `PageHeader` | Breadcrumb + title + action buttons | `breadcrumbs[]`, `title`, `actions[]` |
| `StatusBar` | Bottom bar with server info | `version`, `dbPath`, `uptime` |

### 4.2 Data Display Components

| Component | Description | Props |
|-----------|-------------|-------|
| `DataTable` | Sortable, paginated table with row selection | `columns[]`, `data[]`, `onSort`, `onSelect`, `pagination` |
| `DataTableColumn` | Column definition | `key`, `label`, `sortable`, `width`, `render?` |
| `Pagination` | Page navigation with size selector | `page`, `perPage`, `total`, `onChange` |
| `StatCard` | KPI metric card | `label`, `value`, `trend?`, `icon` |
| `Badge` | Colored label | `text`, `variant: 'default'|'success'|'warning'|'danger'|'info'` |
| `EmptyState` | Placeholder when no data | `icon`, `title`, `description`, `action?` |
| `CodeBlock` | Syntax-highlighted JSON/code display | `code`, `language` |
| `Sparkline` | Inline mini chart (SVG) | `data[]`, `width`, `height`, `color` |

### 4.3 Form Components

| Component | Description | Props |
|-----------|-------------|-------|
| `TextInput` | Standard text field | `label`, `name`, `value`, `placeholder`, `error?`, `required?` |
| `PasswordInput` | Text field with show/hide toggle | `label`, `name`, `value`, `error?` |
| `NumberInput` | Numeric field with min/max | `label`, `name`, `value`, `min?`, `max?` |
| `TextArea` | Multi-line text | `label`, `name`, `value`, `rows` |
| `SelectInput` | Dropdown select | `label`, `name`, `value`, `options[]` |
| `MultiSelect` | Multi-select with tag chips | `label`, `name`, `values[]`, `options[]` |
| `Toggle` | On/off switch | `label`, `checked`, `onChange` |
| `Checkbox` | Checkbox with label | `label`, `checked`, `onChange` |
| `RadioGroup` | Grouped radio buttons | `label`, `name`, `value`, `options[]` |
| `FileUpload` | Drag-and-drop file picker | `label`, `accept`, `maxSize`, `multiple`, `files[]` |
| `TagInput` | Freeform tag entry (for select values) | `label`, `tags[]`, `onChange` |
| `DatePicker` | Date/datetime picker | `label`, `value`, `includeTime?`, `min?`, `max?` |
| `FilterInput` | PocketBase filter syntax input with autocomplete | `value`, `fields[]`, `onChange` |
| `RuleInput` | API rule editor with syntax help | `value`, `onChange`, `placeholder` |
| `JsonEditor` | JSON editing with validation | `value`, `onChange`, `schema?` |

### 4.4 Feedback Components

| Component | Description | Props |
|-----------|-------------|-------|
| `Toast` | Temporary notification | `message`, `type: 'success'|'error'|'info'|'warning'`, `duration?` |
| `ConfirmDialog` | Destructive action confirmation | `title`, `message`, `confirmLabel`, `onConfirm`, `onCancel`, `variant` |
| `LoadingSpinner` | Inline or overlay spinner | `size`, `overlay?` |
| `SkeletonLoader` | Content placeholder while loading | `variant: 'text'|'card'|'table'|'form'` |
| `ErrorBanner` | Inline error message | `message`, `retry?` |
| `ProgressBar` | Determinate progress indicator | `value`, `max` |

### 4.5 Overlay Components

| Component | Description | Props |
|-----------|-------------|-------|
| `Modal` | Centered dialog | `title`, `open`, `onClose`, `children`, `size` |
| `Drawer` | Slide-over panel from right | `title`, `open`, `onClose`, `children`, `width` |
| `DropdownMenu` | Action menu on trigger | `trigger`, `items[]` |
| `Tooltip` | Hover tooltip | `content`, `position`, `children` |
| `CommandPalette` | Global search overlay (`⌘K`) | `open`, `onClose`, `onSelect` |

### 4.6 Specialized Components

| Component | Description | Props |
|-----------|-------------|-------|
| `CollectionList` | Sidebar list of collections with filter | `collections[]`, `selected`, `onSelect` |
| `FieldEditor` | Inline/modal field definition editor | `field?`, `onSave`, `onCancel` |
| `FieldTypeSelector` | Grid of field type options | `value`, `onChange` |
| `FieldRow` | Draggable field display in schema editor | `field`, `onEdit`, `onRemove`, `isDragging` |
| `ApiPreview` | cURL/SDK code examples for a collection | `collection`, `baseUrl` |
| `LogRow` | Expandable log entry with detail panel | `log`, `expanded`, `onToggle` |
| `BackupRow` | Backup entry with download/restore/delete | `backup`, `onRestore`, `onDelete`, `onDownload` |
| `OAuthProviderCard` | OAuth provider config card | `provider`, `enabled`, `onConfigure` |
| `RecordForm` | Auto-generated form from collection schema | `collection`, `record?`, `onSave` |

---

## 5. Navigation Flow

### 5.1 Route Map

```
/_/
├── /_/                              → Dashboard (overview)
├── /_/auth/login                    → Login page (unauthenticated)
├── /_/auth/setup                    → First-run superuser setup
├── /_/collections                   → Collections manager (list + detail)
│   └── /_/collections/:name/records → Records browser for collection
├── /_/settings                      → Settings (tabbed)
│   ├── /_/settings?tab=app          → Application settings
│   ├── /_/settings?tab=mail         → SMTP settings
│   ├── /_/settings?tab=storage      → Storage settings
│   ├── /_/settings?tab=auth         → Auth providers
│   └── /_/settings?tab=backups      → Backup settings
├── /_/logs                          → Logs viewer
└── /_/backups                       → Backups manager
```

### 5.2 User Flows

#### First-Run Flow
```
[Visit /_/] → [No superuser exists] → [/_/auth/setup]
→ [Create email + password] → [Auto-login] → [/_/ Dashboard]
```

#### Standard Login Flow
```
[Visit /_/] → [Not authenticated] → [/_/auth/login]
→ [Enter credentials] → [Validate] → [/_/ Dashboard]
                                └──→ [Error: show inline message]
```

#### Collection Management Flow
```
[Dashboard] → [Click "Collections" in sidebar]
→ [/_/collections] → [Select collection from left panel]
→ [View/edit schema in right panel]
→ [Add field] → [Field editor modal opens]
→ [Configure type + options] → [Save field]
→ [Save collection changes]

Alternative: [Click "View Records"] → [/_/collections/:name/records]
→ [Browse/filter/sort records]
→ [Click record row] → [Record editor drawer slides in]
→ [Edit fields] → [Save record]
```

#### Settings Flow
```
[Dashboard] → [Click "Settings" in sidebar]
→ [/_/settings] → [Default to "Application" tab]
→ [Switch tabs to configure mail/storage/auth/backups]
→ [Edit values] → [Save Changes per tab]
→ [Success toast notification]
```

### 5.3 State Management Architecture

```
stores/
├── auth.ts          → Superuser session (token, user info)
├── collections.ts   → Collection list + selected collection
├── records.ts       → Current record list, filters, pagination
├── settings.ts      → Settings data per category
├── logs.ts          → Log entries, filters
├── ui.ts            → Sidebar state, theme, modals
└── toast.ts         → Toast notification queue
```

Using **nanostores** for lightweight reactive state, persisted to `localStorage` for theme preference and sidebar collapse state.

### 5.4 API Client Architecture

```
lib/
├── api/
│   ├── client.ts        → Base fetch wrapper with auth headers + error handling
│   ├── collections.ts   → GET/POST/PUT/DELETE /api/collections
│   ├── records.ts       → CRUD /api/collections/:name/records
│   ├── settings.ts      → GET/PUT /api/settings
│   ├── logs.ts          → GET /api/logs + GET /api/logs/stats
│   ├── backups.ts       → GET/POST/DELETE /api/backups
│   └── auth.ts          → POST /api/admins/auth-with-password
└── types/
    ├── collection.ts    → Collection, Field, FieldType interfaces
    ├── record.ts        → Record, RecordList interfaces
    ├── settings.ts      → Settings DTOs
    └── log.ts           → LogEntry, LogStats interfaces
```

---

## 6. Interaction Patterns

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `⌘K` / `Ctrl+K` | Open command palette / global search |
| `Escape` | Close modal/drawer/dropdown |
| `⌘S` / `Ctrl+S` | Save current form |
| `⌘N` / `Ctrl+N` | New record (when in records view) |
| `↑` / `↓` | Navigate table rows |
| `Enter` | Open selected row |
| `Delete` / `Backspace` | Delete selected (with confirmation) |

### Toast Notifications

- **Success:** Green, 3s auto-dismiss. "Collection saved", "Record created", etc.
- **Error:** Red, persistent until dismissed. Shows error message from API.
- **Warning:** Amber, 5s auto-dismiss. "Unsaved changes", etc.
- **Info:** Blue, 3s auto-dismiss. "Backup started", etc.

### Loading States

- **Page load:** Skeleton loaders matching content layout
- **Table load:** Skeleton rows (5 rows) with shimmer animation
- **Form submit:** Button shows spinner, disables, label changes to "Saving..."
- **Destructive action:** Confirm dialog → button shows spinner → success/error toast

### Error Handling

- **Form validation:** Inline errors below fields, first error focused
- **API errors:** Toast notification + optional retry action
- **Network errors:** Full-page error banner with retry button
- **401 Unauthorized:** Redirect to login with return URL preserved

---

## 7. Responsive Breakpoints

| Breakpoint | Layout Changes |
|------------|----------------|
| **≥1280px** | Full sidebar (240px) + content. Two-panel collection view. |
| **1024–1279px** | Collapsed sidebar (64px, icons only). Content fills remaining. |
| **768–1023px** | Sidebar hidden (drawer mode). Single-panel collection view. Record editor becomes full-screen modal. |
| **<768px** | Full mobile layout. Stacked forms. Bottom navigation bar replaces sidebar. Simplified table (fewer columns, card layout option). |

---

## 8. Accessibility Considerations

- All interactive elements have `aria-label` or visible labels
- Focus rings visible on keyboard navigation (`focus-visible`)
- Color is never the sole indicator (icons + color for status)
- `prefers-reduced-motion` respected for all animations
- Semantic HTML: `<nav>`, `<main>`, `<aside>`, `<header>`, `<table>`
- Skip-to-main-content link
- All images/icons have `alt` or `aria-hidden="true"`
- Form inputs have associated `<label>` elements
- WCAG 2.1 AA contrast ratios (4.5:1 for text)
- `role="alert"` on toast notifications
- Keyboard-navigable data table (arrow keys, Enter, Escape)

---

## 9. File Structure (AstroJS Frontend)

```
frontend/
├── package.json
├── astro.config.mjs
├── tsconfig.json
├── public/
│   ├── favicon.ico
│   └── fonts/
│       ├── GeneralSans-Variable.woff2
│       └── JetBrainsMono-Variable.woff2
├── src/
│   ├── styles/
│   │   └── global.css                  # CSS variables, base styles, Tailwind
│   ├── layouts/
│   │   ├── AdminLayout.astro           # AppShell (topbar + sidebar + content)
│   │   └── AuthLayout.astro            # Centered card layout for login/setup
│   ├── components/
│   │   ├── layout/
│   │   │   ├── Sidebar.astro
│   │   │   ├── SidebarItem.astro
│   │   │   ├── Topbar.astro
│   │   │   ├── PageHeader.astro
│   │   │   └── StatusBar.astro
│   │   ├── data/
│   │   │   ├── DataTable.astro
│   │   │   ├── Pagination.astro
│   │   │   ├── StatCard.astro
│   │   │   ├── Badge.astro
│   │   │   ├── EmptyState.astro
│   │   │   ├── CodeBlock.astro
│   │   │   └── Sparkline.astro
│   │   ├── form/
│   │   │   ├── TextInput.astro
│   │   │   ├── PasswordInput.astro
│   │   │   ├── NumberInput.astro
│   │   │   ├── TextArea.astro
│   │   │   ├── SelectInput.astro
│   │   │   ├── MultiSelect.astro
│   │   │   ├── Toggle.astro
│   │   │   ├── Checkbox.astro
│   │   │   ├── RadioGroup.astro
│   │   │   ├── FileUpload.astro
│   │   │   ├── TagInput.astro
│   │   │   ├── DatePicker.astro
│   │   │   ├── FilterInput.astro
│   │   │   ├── RuleInput.astro
│   │   │   └── JsonEditor.astro
│   │   ├── feedback/
│   │   │   ├── Toast.astro
│   │   │   ├── ConfirmDialog.astro
│   │   │   ├── LoadingSpinner.astro
│   │   │   ├── SkeletonLoader.astro
│   │   │   ├── ErrorBanner.astro
│   │   │   └── ProgressBar.astro
│   │   ├── overlay/
│   │   │   ├── Modal.astro
│   │   │   ├── Drawer.astro
│   │   │   ├── DropdownMenu.astro
│   │   │   ├── Tooltip.astro
│   │   │   └── CommandPalette.astro
│   │   └── specialized/
│   │       ├── CollectionList.astro
│   │       ├── FieldEditor.astro
│   │       ├── FieldTypeSelector.astro
│   │       ├── FieldRow.astro
│   │       ├── ApiPreview.astro
│   │       ├── LogRow.astro
│   │       ├── BackupRow.astro
│   │       ├── OAuthProviderCard.astro
│   │       └── RecordForm.astro
│   ├── pages/
│   │   ├── _/
│   │   │   ├── index.astro             # Dashboard
│   │   │   ├── collections/
│   │   │   │   ├── index.astro         # Collections manager
│   │   │   │   └── [name]/
│   │   │   │       └── records.astro   # Records browser
│   │   │   ├── settings.astro          # Settings (tabbed)
│   │   │   ├── logs.astro              # Logs viewer
│   │   │   ├── backups.astro           # Backups manager
│   │   │   └── auth/
│   │   │       ├── login.astro         # Login page
│   │   │       └── setup.astro         # First-run setup
│   ├── stores/
│   │   ├── auth.ts
│   │   ├── collections.ts
│   │   ├── records.ts
│   │   ├── settings.ts
│   │   ├── logs.ts
│   │   ├── ui.ts
│   │   └── toast.ts
│   ├── lib/
│   │   ├── api/
│   │   │   ├── client.ts
│   │   │   ├── collections.ts
│   │   │   ├── records.ts
│   │   │   ├── settings.ts
│   │   │   ├── logs.ts
│   │   │   ├── backups.ts
│   │   │   └── auth.ts
│   │   └── types/
│   │       ├── collection.ts
│   │       ├── record.ts
│   │       ├── settings.ts
│   │       └── log.ts
│   └── utils/
│       ├── cn.ts                       # Tailwind class merge utility
│       ├── format.ts                   # Date, number, byte formatters
│       └── shortcuts.ts               # Keyboard shortcut manager
```

---

## 10. Design Tokens Summary

### Typography

| Usage | Font | Weight | Size |
|-------|------|--------|------|
| Page title | General Sans | 600 | 24px / 1.5rem |
| Section heading | General Sans | 600 | 18px / 1.125rem |
| Card title | General Sans | 500 | 16px / 1rem |
| Body text | General Sans | 400 | 14px / 0.875rem |
| Label / caption | General Sans | 500 | 12px / 0.75rem |
| Code / IDs | JetBrains Mono | 400 | 13px / 0.8125rem |
| Stat number | General Sans | 700 | 32px / 2rem |

### Shadows

```
--shadow-sm:  0 1px 2px 0 rgba(0,0,0,0.3);
--shadow-md:  0 4px 6px -1px rgba(0,0,0,0.3);
--shadow-lg:  0 10px 15px -3px rgba(0,0,0,0.3);
--shadow-xl:  0 20px 25px -5px rgba(0,0,0,0.3);
```

### Transitions

```
--transition-fast: 150ms ease-out;
--transition-base: 200ms ease-out;
--transition-slow: 300ms ease-out;
```

### Z-Index Scale

```
--z-base:     0;
--z-dropdown: 10;
--z-sticky:   20;
--z-drawer:   30;
--z-modal:    40;
--z-toast:    50;
--z-tooltip:  60;
```
