# Zerobase Admin — Component Inventory

**Total components: 53**
**Categories: 6**

---

## Component Dependency Graph

```
AppShell (root)
├── Topbar
│   ├── CommandPalette
│   ├── DropdownMenu (user menu)
│   └── Toggle (theme)
├── Sidebar
│   └── SidebarItem (×6)
├── StatusBar
└── [Page Content]
    ├── PageHeader
    │   └── Badge (breadcrumbs)
    └── [Page-specific components]
```

---

## 1. Layout Components (6)

### `AppShell`
- **Purpose:** Root application layout with sidebar, topbar, content area, status bar
- **Client-side JS:** Sidebar collapse state, theme toggle
- **Variants:** Full sidebar | collapsed sidebar | mobile drawer
- **Used on:** Every authenticated page

### `Sidebar`
- **Purpose:** Primary navigation panel
- **State:** Collapsed (boolean), stored in localStorage
- **Children:** SidebarItem components
- **Responsive:** Drawer mode below 768px

### `SidebarItem`
- **Purpose:** Individual navigation link
- **Visual states:** Default, hover, active (current route)
- **Active indicator:** Left accent border + tinted background
- **Props:** `icon: string`, `label: string`, `href: string`, `badge?: number`

### `Topbar`
- **Purpose:** Application header bar
- **Fixed:** `position: sticky; top: 0; z-index: 20`
- **Content:** Logo, global search trigger, theme toggle, user dropdown

### `PageHeader`
- **Purpose:** Page title row with breadcrumbs and action buttons
- **Pattern:** `Breadcrumb > Title` on left, action buttons on right
- **Used on:** Every content page

### `StatusBar`
- **Purpose:** Persistent footer showing server metadata
- **Content:** Version, SQLite path, uptime, connection status
- **Update:** Polls server health endpoint every 30s

---

## 2. Data Display Components (8)

### `DataTable`
- **Purpose:** Primary data display for records and logs
- **Features:**
  - Sortable columns (click header to cycle asc/desc/none)
  - Resizable column widths (drag border)
  - Row selection (checkbox column)
  - Row click handler (open detail)
  - Empty state fallback
  - Skeleton loading state
- **Pagination:** Delegates to `Pagination` component
- **Accessibility:** `<table>` with proper `<thead>`, `<th scope="col">`, keyboard arrow navigation

### `Pagination`
- **Purpose:** Page navigation below data tables
- **Display:** `← 1 2 3 ... N →` with page size selector
- **Page sizes:** 10, 25, 50, 100
- **State synced to URL:** `?page=2&perPage=25`

### `StatCard`
- **Purpose:** KPI metric display on dashboard
- **Layout:** Icon (top-left), label, large number, trend delta
- **Trend:** Green arrow-up for positive, red arrow-down for negative
- **Hover:** Subtle elevation shadow increase

### `Badge`
- **Purpose:** Inline colored label
- **Variants:** `default` (gray), `success` (green), `warning` (amber), `danger` (red), `info` (blue), `auth` (purple), `base` (gray), `view` (teal)
- **Usage:** Collection types, field constraints, log status codes

### `EmptyState`
- **Purpose:** Placeholder when a list/table has zero items
- **Content:** Illustration/icon, headline, description, optional CTA button
- **Usage:** Empty collections, no records found, no logs matching filter

### `CodeBlock`
- **Purpose:** Display formatted JSON or code snippets
- **Features:** Line numbers, copy button, syntax highlighting (CSS-only via token classes)
- **Usage:** API preview, log request/response bodies, filter examples

### `Sparkline`
- **Purpose:** Inline mini chart for trends
- **Implementation:** Pure SVG, no chart library
- **Props:** `data: number[]`, `width`, `height`, `color`, `fill?: boolean`
- **Usage:** Dashboard request volume, stat card trends

### `KeyValueList`
- **Purpose:** Display key-value pairs (log metadata, record system fields)
- **Layout:** Two-column grid: label (muted) | value
- **Usage:** Log detail panel, record metadata section

---

## 3. Form Components (15)

### `TextInput`
- **States:** Default, focused, error, disabled
- **Error:** Red border + inline error message below
- **Props:** `label`, `name`, `value`, `placeholder`, `error?`, `required?`, `autocomplete?`

### `PasswordInput`
- **Extends:** TextInput with visibility toggle button (eye icon)
- **Additional:** `type` toggles between "password" and "text"

### `NumberInput`
- **Additional props:** `min?`, `max?`, `step?`, `noDecimal?`
- **Validation:** Prevents non-numeric input, enforces min/max

### `TextArea`
- **Auto-resize:** Grows with content up to `maxRows`
- **Props:** `label`, `name`, `value`, `rows`, `maxRows?`, `placeholder`

### `SelectInput`
- **Implementation:** Native `<select>` styled with Tailwind (consistent cross-browser)
- **Props:** `label`, `name`, `value`, `options: { label: string, value: string }[]`
- **Variant:** With search filter for long option lists

### `MultiSelect`
- **Display:** Selected values as removable chips/tags
- **Dropdown:** Checkbox list with search filter
- **Props:** `label`, `name`, `values: string[]`, `options: string[]`, `maxSelect?`

### `Toggle`
- **Purpose:** Boolean on/off switch
- **Semantics:** `<input type="checkbox" role="switch">`
- **Animation:** Smooth slide with color transition (200ms)
- **Usage:** Enable/disable SMTP, OAuth providers, auto-backup

### `Checkbox`
- **Purpose:** Boolean with label
- **Semantics:** `<input type="checkbox">` with `<label>`
- **Usage:** Field constraints (required, unique), TLS toggle

### `RadioGroup`
- **Purpose:** Single selection from group
- **Layout:** Vertical or horizontal arrangement
- **Usage:** Storage backend selection (local/S3)

### `FileUpload`
- **Features:**
  - Drag-and-drop zone with visual feedback
  - Click to browse
  - File type validation (accept attribute)
  - Max size validation
  - Preview thumbnails for images
  - Multiple file support
  - Upload progress bar
- **States:** Empty, dragging over, files selected, uploading, error

### `TagInput`
- **Purpose:** Freeform tag entry (e.g., select field values, MIME types)
- **Behavior:** Type + Enter to add tag, click X to remove
- **Display:** Tags as removable chips in input area

### `DatePicker`
- **Implementation:** Native `<input type="date">` / `<input type="datetime-local">`
- **Props:** `label`, `value`, `includeTime?`, `min?`, `max?`
- **Fallback:** Custom picker for browsers with poor native support

### `FilterInput`
- **Purpose:** PocketBase filter syntax input
- **Features:**
  - Autocomplete for field names (`@request.auth.*`, collection fields)
  - Syntax highlighting for operators (`=`, `!=`, `~`, `>`, `<`)
  - Validation feedback (parse success/error)
- **Usage:** Records browser filter bar, log filter

### `RuleInput`
- **Purpose:** API access rule editor
- **Features:** Same as FilterInput + helper text explaining empty vs `""` rules
- **Usage:** Collection API rules (list/view/create/update/delete)

### `JsonEditor`
- **Purpose:** JSON value editing with validation
- **Features:**
  - Monospace font (JetBrains Mono)
  - Auto-indentation
  - JSON parse validation on blur
  - Error indicator for invalid JSON
- **Usage:** JSON field type in record editor

---

## 4. Feedback Components (6)

### `Toast`
- **Position:** Top-right, stacked (max 3 visible)
- **Variants:** `success` (green), `error` (red), `warning` (amber), `info` (blue)
- **Behavior:** Auto-dismiss (3s default, persistent for errors), manual dismiss
- **Animation:** Slide in from right, fade out
- **Accessibility:** `role="alert"`, `aria-live="polite"`

### `ConfirmDialog`
- **Purpose:** Confirmation before destructive actions
- **Content:** Title, description, cancel button, confirm button
- **Confirm button variants:** Danger (red) for delete, primary for restore
- **Focus:** Confirm button focused on open, Escape to cancel
- **Usage:** Delete record, delete collection, restore backup, delete backup

### `LoadingSpinner`
- **Sizes:** `sm` (16px), `md` (24px), `lg` (40px)
- **Variants:** Inline (within button), overlay (centered on container)
- **Animation:** CSS `@keyframes spin` (respects prefers-reduced-motion)

### `SkeletonLoader`
- **Variants:**
  - `text`: Single line shimmer
  - `card`: Card-shaped block
  - `table`: Multiple rows with column widths
  - `form`: Stacked label + input pairs
- **Animation:** Shimmer gradient sweep (left to right)

### `ErrorBanner`
- **Purpose:** Inline error message for section-level failures
- **Content:** Error icon + message + optional retry button
- **Position:** Top of content section where error occurred

### `ProgressBar`
- **Purpose:** Determinate progress (file upload, backup creation)
- **Display:** Horizontal bar with percentage label
- **Animation:** Width transition (200ms ease-out)

---

## 5. Overlay Components (5)

### `Modal`
- **Sizes:** `sm` (400px), `md` (560px), `lg` (720px), `xl` (960px)
- **Behavior:**
  - Centered on screen with backdrop overlay
  - Escape to close
  - Click backdrop to close (configurable)
  - Focus trap (tab cycles within modal)
  - Scroll lock on body
- **Animation:** Fade in + scale up (150ms)

### `Drawer`
- **Direction:** Right-to-left slide
- **Width:** 480px default, full-screen on mobile
- **Behavior:** Same as Modal (escape, backdrop click, focus trap)
- **Usage:** Record editor, API preview, field editor

### `DropdownMenu`
- **Trigger:** Button click (not hover)
- **Position:** Auto-positioned below trigger, flips if near viewport edge
- **Items:** Text items, dividers, danger items (red)
- **Keyboard:** Arrow keys to navigate, Enter to select, Escape to close

### `Tooltip`
- **Trigger:** Hover (desktop) or long-press (mobile)
- **Position:** Auto-positioned (top/bottom/left/right)
- **Delay:** 300ms show delay, 100ms hide delay
- **Usage:** Icon-only buttons, truncated text, help hints

### `CommandPalette`
- **Trigger:** `⌘K` / `Ctrl+K`
- **Features:**
  - Text search input with auto-focus
  - Result categories: Collections, Records, Settings, Actions
  - Keyboard navigation (arrow keys + Enter)
  - Recent searches
- **Animation:** Fade in + slight scale (100ms)

---

## 6. Specialized Components (13)

### `CollectionList`
- **Purpose:** Left panel in collections manager
- **Features:**
  - Search/filter input at top
  - Collection items with name + type badge
  - Click to select (highlights active)
  - "New Collection" button at bottom
- **Sorting:** Alphabetical by name

### `FieldEditor`
- **Purpose:** Add or edit a field definition
- **Presentation:** Modal (on add) or inline expansion (on edit)
- **Content:**
  - Field name input
  - Field type selector
  - Type-specific options panel (dynamic based on selected type)
  - Constraint checkboxes (required, unique, searchable)
- **Validation:** Name format (alphanumeric + underscore), type-specific option validation

### `FieldTypeSelector`
- **Purpose:** Visual grid of available field types
- **Layout:** 3×5 grid of type cards
- **Each card:** Icon + type name
- **Types (15):** Text, Number, Bool, Email, URL, DateTime, AutoDate, Select, MultiSelect, File, Relation, JSON, Editor, Password

### `FieldRow`
- **Purpose:** Single field in the schema field list
- **Layout:** `[≡ drag] [name] [type badge] [constraint badges] [edit] [delete]`
- **Drag:** Handle on left for reordering (drag-and-drop)
- **System fields:** Dimmed, no drag/edit/delete controls
- **Hover:** Subtle background highlight

### `ApiPreview`
- **Purpose:** Show API endpoints and example requests for a collection
- **Content:**
  - List endpoint: `GET /api/collections/:name/records`
  - View endpoint: `GET /api/collections/:name/records/:id`
  - Create, Update, Delete examples
  - cURL commands with auth header
- **Presentation:** Drawer or collapsible panel with CodeBlock

### `LogRow`
- **Purpose:** Single log entry in the logs table
- **Compact view:** Time, method, path, status (color-coded), duration
- **Expanded view:** Full request/response details (IP, auth, headers, bodies)
- **Toggle:** Click row to expand/collapse
- **Color coding:** 2xx green, 3xx blue, 4xx amber, 5xx red

### `BackupRow`
- **Purpose:** Single backup entry in the backups list
- **Content:** Filename, size, date, action buttons
- **Actions:**
  - Download (↓ icon)
  - Restore (opens confirm dialog — destructive!)
  - Delete (opens confirm dialog)

### `OAuthProviderCard`
- **Purpose:** OAuth provider configuration card
- **Content:** Provider icon/logo, name, enabled status, configure button
- **Expanded:** Client ID, Client Secret, Redirect URL (read-only + copy)
- **Providers:** Google, Microsoft, GitHub (extensible)

### `RecordForm`
- **Purpose:** Auto-generated form from collection schema
- **Dynamic:** Reads collection fields and renders appropriate form component for each
- **Field type → Component mapping:**

| FieldType | Form Component |
|-----------|----------------|
| Text | TextInput |
| Number | NumberInput |
| Bool | Toggle |
| Email | TextInput (type="email") |
| URL | TextInput (type="url") |
| DateTime | DatePicker (includeTime=true) |
| AutoDate | Read-only display |
| Select | SelectInput |
| MultiSelect | MultiSelect |
| File | FileUpload |
| Relation | SelectInput (with collection records as options) |
| JSON | JsonEditor |
| Editor | TextArea (rich text in future) |
| Password | PasswordInput |

### `CollectionTypeIcon`
- **Purpose:** Visual indicator for collection type
- **Icons:**
  - Base: Table icon (gray)
  - Auth: Shield icon (purple)
  - View: Eye icon (teal)

### `ThemeToggle`
- **Purpose:** Switch between light and dark themes
- **Icon:** Sun (light) / Moon (dark) with rotation animation
- **Persistence:** localStorage + system preference detection

### `SearchInput`
- **Purpose:** Reusable search field with icon and clear button
- **Features:** Debounced input (300ms), clear button when non-empty
- **Usage:** Collection list filter, record browser filter, log filter

### `CopyButton`
- **Purpose:** Copy text to clipboard with feedback
- **States:** Default (copy icon) → Copied (check icon, 2s) → Default
- **Usage:** Redirect URLs, API endpoints, record IDs
