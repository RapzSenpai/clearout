# ClearOut — Blueprint

Deep Windows uninstaller. Free, open-source, lightweight. Rust + Tauri + Svelte + Bits UI.

---

## 0. Project Overview

**What it does:** Uninstalls Windows apps via native uninstaller, then deep-scans for leftover files/registry/services/startup entries, scores confidence, lets user review + delete safely.

**Non-goals:** No telemetry, no bundled offers, no auto-delete without confirmation, no cloud AI by default, no database.

**Target platform:** Windows 10/11, x64 only.

---

## 1. UI Library Decision — Bits UI (not shadcn-svelte)

**Choice: Bits UI alone.**

Bits UI is headless — unstyled primitives (Dialog, Tabs, Checkbox, Tooltip, Dropdown, etc.) for Svelte, built on Melt UI. Zero default visual style, full ARIA/keyboard handling built in.

shadcn-svelte actually wraps Bits UI internally, but ships a pre-baked Tailwind theme (rounded-xl, default color scale) on top — that's the generic-AI-app look we're avoiding. Since we're building a custom minimalist single-accent theme from scratch, adding shadcn-svelte's theme layer just means overriding it. Skip it — use Bits UI primitives directly, apply our own tokens.

```
npm install bits-ui
```

---

## 2. Design System

### 2.1 Theme: light, smoky white, single accent

```css
:root {
  --bg: #F4F4F2;              /* smoky white */
  --surface: #FFFFFF;         /* cards/rows on top of bg */
  --border: #E4E4E1;
  --text-primary: #1A1A18;
  --text-secondary: #6B6B67;
  --accent: #3ECF8E;          /* soft green — primary actions, high confidence, links */
  --accent-soft: #E8F8F0;     /* accent tint for subtle backgrounds/hover */
  --danger: #D64545;          /* reserved ONLY for Force Kill button — functional exception */
  --font-ui: 'Inter', system-ui, sans-serif;
  --font-mono: 'JetBrains Mono', monospace;
}
```

**Rule:** one accent color (`--accent`) for everything positive/primary/interactive. `--danger` exists solely for the Force Kill action — nowhere else. No amber, no blue, no multi-color badge system.

### 2.2 Confidence indicator (single-color system)

Since no red/amber/green traffic-light system — confidence shown via **weight + fill**, not multiple hues:

| Tier | Visual |
|---|---|
| High | Solid `--accent` filled badge, text "High" |
| Medium | Outlined `--accent` badge (border only, no fill), text "Medium" |
| Low | Gray (`--text-secondary`) outlined badge, text "Low", row collapsed by default |

### 2.3 Typography

- UI/body text: Inter
- File paths, registry keys, technical data: JetBrains Mono
- Base size 13-14px in dense tables, 15-16px elsewhere

### 2.4 Buttons

Small (32-36px height), 4-6px radius, no pills, no shadows — 1px borders for separation. Primary = solid `--accent` bg, white text. Secondary/ghost = transparent bg, `--border` outline. Destructive (Force Kill only) = `--danger` bg.

### 2.5 Bits UI → screen component mapping

| Bits UI primitive | Used in |
|---|---|
| `Tabs` | Leftover Review category tabs (Files, Registry, Services, Startup) |
| `Dialog` | Uninstall Confirmation, Locked Item Handling |
| `Checkbox` | Leftover item selection, "Create restore point" toggle |
| `Tooltip` | Truncated file path hover-to-reveal full path |
| `DropdownMenu` | Sort options on Dashboard, provider select in Settings |
| `Switch` | Settings toggles (AI enable, force-kill allow, restore point default) |
| `Progress` | Scan progress bar |
| `Select` | Scan depth (fast/thorough), AI provider |
| `Toast` (if available in version used, else custom) | Success/error notifications |

All above styled via our tokens (2.1) — Bits UI provides behavior only, zero visual opinion.

---

## 3. Pages & Features

### 3.1 Dashboard
- Installed app table: icon, name, publisher, size, install date
- Search bar
- Sort (name/size/date) — `DropdownMenu`
- Stat chips: total apps, total size
- Uninstall button per row

### 3.2 Uninstall Confirmation (`Dialog`)
- App details: name, version, publisher, install path
- Uninstall button → inline progress state

### 3.3 Scanning (inline state, not a separate route)
- `Progress` bar
- Live count per category

### 3.4 Leftover Review
- `Tabs`: Files/Folders, Registry, Services, Startup
- Table: `Checkbox`, path (mono, `Tooltip` on truncation), type icon, confidence badge (2.2), Ask AI icon-button
- Bulk actions: Select All High Confidence, Select All, Clear
- Sticky bottom bar: selected count, restore point `Checkbox`, Delete button

### 3.5 Locked Item Handling (`Dialog`, conditional)
- Process list holding locks
- Close (graceful) / Force Kill (`--danger`, explicit second confirm) per process

### 3.6 Result Summary
- Deleted/skipped counts, restore point ID
- Export Report button
- Run Verification Scan button

### 3.7 Settings
- AI: `Switch` enable, `Select` provider, API key input
- Force-kill: `Switch` allow
- Restore point: `Switch` default on/off
- Scan depth: `Select` fast/thorough

---

## 4. Flow

```
Dashboard
   │ click Uninstall
   ▼
Uninstall Confirmation (Dialog) → run native uninstaller
   │ exit code captured
   ▼
Scanning (inline progress)
   │ scan complete
   ▼
Leftover Review → user checks items → click Delete
   │
   ├─ locks detected? → Locked Item Handling (Dialog) → resolve → continue
   │
   ▼
Restore point created → items deleted
   ▼
Result Summary → optional Export / Verify
   ▼
back to Dashboard
```

Settings reachable anytime via sidebar, outside the linear flow.

---

## 5. Architecture

```
┌─────────────────────────────┐
│  Svelte + Bits UI Frontend   │
│  (Dashboard, Review, Settings)│
└──────────────┬───────────────┘
               │ invoke() / IPC (Tauri commands)
┌──────────────▼───────────────┐
│   Rust Backend (src-tauri)    │
│  ┌─────────────────────────┐ │
│  │ Commands (thin handlers) │ │
│  └───────────┬─────────────┘ │
│  ┌───────────▼─────────────┐ │
│  │ Engine (business logic)  │ │
│  │ - registry_scan           │ │
│  │ - file_scan                │ │
│  │ - service_scan              │ │
│  │ - startup_scan               │ │
│  │ - confidence scoring          │ │
│  └───────────┬─────────────┘ │
│  ┌───────────▼─────────────┐ │
│  │ windows-rs (Registry/SCM/ │ │
│  │ Restart Manager APIs)     │ │
│  └───────────────────────────┘ │
└───────────────────────────────┘
```

No database. Local `%APPDATA%\ClearOut\config.json` for settings only.

### 5.1 Data flow

1. `get_installed_apps()` → registry read → `AppInfo[]` → Dashboard table
2. Uninstall click → `run_uninstaller(app_id)` → spawn `UninstallString` → capture exit code
3. `scan_leftovers(app_info)` → parallel scan (files/registry/services/startup) → merge, dedupe, score → `LeftoverItem[]`
4. Leftover Review renders grouped/scored results, user selects
5. `delete_items(selected, create_restore_point)` → `check_locks()` first → if clear, restore point → delete → `DeleteResult`
6. Optional `verify_scan(app_info)` re-runs step 3, diffs against deleted set

### 5.2 Confidence scoring formula

- +40: path contains exact app name/publisher string
- +30: path under app's recorded `InstallLocation` (captured pre-uninstall)
- +20: fuzzy match (Jaro-Winkler, `strsim`) > 0.85
- −50: matches protected-path whitelist (auto-exclude regardless of score)
- −30: generic/stoplisted term match ("data", "cache", "settings" alone)
- Score ≥ 70 → High · 40–69 → Medium · < 40 → Low

Protected-path whitelist: `C:\Windows`, `C:\Windows\System32`, other vendors' folders unless exact full-path match.

---

## 6. Tech Stack

**Core:** Rust, Tauri 2, `windows` crate (Registry/Services/RestartManager/FileSystem/Restore features), `winreg`, `strsim`, `walkdir`, `serde`/`serde_json`, `reqwest` + `tokio` (AI feature only)

**Frontend:** Svelte, TypeScript, Tailwind (utility classes only, no default theme), Bits UI

**Fonts:** Inter (UI), JetBrains Mono (technical data)

**Build/dist:** Tauri bundler → portable `.exe`, GitHub Actions CI on `windows-latest`, GitHub Releases

---

## 7. Project Structure

```
clearout/
├── src/
│   ├── routes/
│   │   ├── Dashboard.svelte
│   │   ├── LeftoverReview.svelte
│   │   ├── Settings.svelte
│   ├── lib/
│   │   ├── components/
│   │   │   ├── AppRow.svelte
│   │   │   ├── ConfidenceBadge.svelte
│   │   │   ├── LeftoverTable.svelte
│   │   │   ├── AskAIPanel.svelte
│   │   │   ├── Button.svelte
│   │   │   ├── UninstallDialog.svelte
│   │   │   ├── LockedItemDialog.svelte
│   │   ├── stores/
│   │   │   ├── apps.ts
│   │   │   ├── scanResults.ts
│   │   │   ├── settings.ts
│   │   ├── tauri-api.ts
│   ├── app.css                # tokens from Section 2.1
│   ├── App.svelte
├── src-tauri/
│   ├── src/
│   │   ├── main.rs
│   │   ├── commands/
│   │   │   ├── mod.rs
│   │   │   ├── inventory.rs
│   │   │   ├── uninstall.rs
│   │   │   ├── scan.rs
│   │   │   ├── delete.rs
│   │   │   ├── locks.rs
│   │   │   ├── ai.rs
│   │   ├── engine/
│   │   │   ├── registry_scan.rs
│   │   │   ├── file_scan.rs
│   │   │   ├── service_scan.rs
│   │   │   ├── startup_scan.rs
│   │   │   ├── confidence.rs
│   │   ├── models.rs
│   ├── Cargo.toml
│   ├── tauri.conf.json
├── tailwind.config.js
├── package.json
```

---

## 8. Implementation Phases

Build strictly in order. Do not start a phase until the previous one's acceptance criteria pass. Feed the AI agent one phase at a time.

### Phase 0 — Environment & Tooling
**Build:**
```
winget install Rustlang.Rustup
rustup default stable-msvc
winget install OpenJS.NodeJS.LTS
winget install Microsoft.VisualStudio.2022.BuildTools
# select "Desktop development with C++" during install
```
```
npm create tauri-app@latest clearout
# choose Svelte, TypeScript, npm
cd clearout
npm install
npm install bits-ui
npm install -D tailwindcss postcss autoprefixer
npx tailwindcss init -p
```
**Test:** `npm run tauri dev` opens blank window, no console errors.
**Acceptance:** Window opens, hot-reload works on file edit.

### Phase 1 — Structure & Design Tokens
**Build:** Create folders from Section 7. Add `app.css` with tokens from 2.1. Configure Tailwind content paths.
**Files:** `src/app.css`, `tailwind.config.js`, empty component files.
**Test:** App renders smoky-white background, Inter font loaded.
**Acceptance:** Blank Dashboard shell with correct bg color, no layout needed yet.

### Phase 2 — Static UI Shell (Bits UI, mock data)
**Build:** All 4 route screens (Dashboard, Leftover Review, Settings, plus dialogs) as static Svelte components using Bits UI primitives per 2.5, hardcoded mock data. `ConfidenceBadge.svelte` implementing the 2.2 single-color tier system. `Button.svelte` per 2.4.
**Dependencies:** Phase 1.
**Test:** Click through every screen with mock data; visually matches Section 2 tokens — one accent color only, red appears solely on Force Kill button.
**Acceptance:** Every screen from Section 3 exists, styled, navigable, zero backend calls yet.

### Phase 3 — App Inventory Engine
**Build:** `inventory.rs` — read `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall` (both native and WOW6432Node views) + `HKCU` equivalent. Parse `DisplayName`, `DisplayVersion`, `Publisher`, `InstallLocation`, `EstimatedSize`, `UninstallString`, `InstallDate`. Filter entries with no `DisplayName`. Wire `get_installed_apps` to Dashboard.
**Files:** `commands/inventory.rs`, `models.rs` (`AppInfo`), `lib/tauri-api.ts`.
**Dependencies:** Phase 2.
**Test:** Compare app list/count against Windows "Apps & Features" on a real machine/VM.
**Acceptance:** Dashboard shows real apps, search/sort work against real data.

### Phase 4 — Uninstall Trigger
**Build:** `uninstall.rs` — parse `UninstallString` (plain exe or `msiexec /x {GUID}`), spawn process, capture exit code. Wire `UninstallDialog.svelte` to real call.
**Files:** `commands/uninstall.rs`.
**Dependencies:** Phase 3.
**Test:** Uninstall a disposable test app in a VM; confirm it disappears from re-scanned inventory.
**Acceptance:** Native uninstaller launches for both EXE and MSI forms; exit code captured correctly.

### Phase 5 — Leftover Scan Engine
**Build:** `registry_scan.rs`, `file_scan.rs`, `service_scan.rs`, `startup_scan.rs`, `confidence.rs` per Section 5.2. Capture app name + `InstallLocation` pre-uninstall (pass through from Phase 4). Orchestrate via `scan.rs` with `tokio::join!`.
**Files:** `src-tauri/src/engine/*`.
**Dependencies:** Phase 4.
**Test:** Scan test app from Phase 4 — verify real leftovers found with sensible scores. Run against an unrelated app name to confirm no false-positive matches.
**Acceptance:** Scored, categorized results returned; no protected-path items appear; false-positive test passes.

### Phase 6 — Leftover Review UI (real data)
**Build:** Wire `LeftoverReview.svelte` to `scan_leftovers`. Implement `Tabs` categories, `Checkbox` pre-selection (High pre-checked), bulk actions, sticky bottom bar.
**Dependencies:** Phase 5.
**Test:** Full flow: uninstall → scan → review shows correct real, categorized, pre-checked data.
**Acceptance:** End-to-end uninstall→scan→review works, no mock data remaining.

### Phase 7 — Lock Detection & Deletion Engine
**Build:** `locks.rs` (Restart Manager API), `delete.rs` (`SRSetRestorePointW` restore point, then per-item delete with partial-failure handling). Wire `LockedItemDialog.svelte` and delete confirmation flow.
**Files:** `commands/locks.rs`, `delete.rs`.
**Dependencies:** Phase 6.
**Test:** Deliberately lock a test file, attempt delete, confirm detection + correct process name reported. Verify restore point created (check via System Restore UI). Confirm Force Kill only fires on explicit second confirm.
**Acceptance:** Locks detected accurately; restore point verifiably created; protected paths never deleted even if selected.

### Phase 8 — Verification Scan & Report Export
**Build:** `verify_scan` reuses Phase 5 engine post-delete, diffs against deleted set. JSON/txt export.
**Dependencies:** Phase 7.
**Test:** Full run; verification scan shows 0 leftovers or correctly flags delete failures (e.g. permissions).
**Acceptance:** Result Summary accurate; exported report valid JSON + readable txt.

### Phase 9 — AI Advisory (opt-in)
**Build:** `ai.rs` — `reqwest` call to user-selected free-tier provider, send metadata only (path, name, size, type, associated app — never file contents). Wire `AskAIPanel.svelte`, Settings `Switch`/`Select`/key input.
**Dependencies:** Phase 6.
**Test:** Valid key → round-trip works, UI labeled "AI assessment" not "guarantee." No key → button hidden/disabled, no error.
**Acceptance:** Fully opt-in, graceful degradation with no key, correct disclaimer present.

### Phase 10 — Settings & Persistence
**Build:** `%APPDATA%\ClearOut\config.json` read/write, `get_settings`/`save_settings`, wire `Settings.svelte`.
**Dependencies:** Phase 9.
**Test:** Change settings, restart app, confirm persisted.
**Acceptance:** All toggles persist; missing/corrupted config falls back to safe defaults without crashing.

### Phase 11 — Testing & Hardening
**Build/Test:**
- Unit (`cargo test`): confidence scoring edge cases, registry parsing on malformed keys, protected-path rejection even on fabricated high scores
- Integration: full inventory→scan pipeline on known test apps in a disposable VM
- Edge cases: app with no `UninstallString`, near-duplicate app names (no cross-contamination), user-cancelled uninstall (non-zero exit handled), disk full during restore point, running without admin rights
- Error scenarios: registry access denied → specific remediation message; antivirus blocking delete → detect `ERROR_ACCESS_DENIED`, surface clearly
**Acceptance:** All edge cases handled without crash; `cargo clippy` clean.

### Phase 12 — Packaging & Distribution
**Build:**
```
npm run tauri build
```
Set `productName`, `identifier` (e.g. `com.rapzzzzz.clearout`), proper `.ico`, disable devtools in release. GitHub Actions on `windows-latest`, triggered on tag push:
```yaml
on:
  push:
    tags: ['v*']
jobs:
  build:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rs/toolchain@v1
        with: { toolchain: stable }
      - run: npm install
      - run: npm run tauri build
      - uses: softprops/action-gh-release@v2
        with:
          files: src-tauri/target/release/bundle/**/*.exe
```
**Acceptance:** Tagged push produces downloadable `.exe` attached to a GitHub Release automatically.

### Phase 13 — Final Polish
- [ ] Performance: profile scan time on 100+ installed apps, confirm UI stays responsive (async scans)
- [ ] Visual consistency pass: confirm single-accent rule held everywhere, `--danger` only on Force Kill
- [ ] Accessibility: full keyboard-only pass, screen reader spot-check
- [ ] Security review: confirm zero network calls unless AI explicitly enabled
- [ ] Code cleanup: remove dead code/unused deps
- [ ] `README.md`: what it does, screenshots, download link, build instructions, safety disclaimer, SmartScreen warning explanation (unsigned exe)
- [ ] `LICENSE`: MIT or GPL-3.0
- [ ] Final clean-VM install test on the actual built release before publishing
