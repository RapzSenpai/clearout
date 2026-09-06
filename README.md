# ⚡ ClearOut

A deep Windows uninstaller with a terminal-console soul. It runs an app's own uninstaller (EXE or MSI), then scans for the files, registry keys, services, and startup entries left behind — lets you review everything with confidence scores, and removes it safely with reversible backups.

**Free. Open-source. Fast. No ads. No bloatware. No telemetry.**

> **Status:** v0.1.0 · Windows 10/11 only · beta — test on disposable apps first.

---

## Features

- **Installed-app inventory** — live list from Windows registry with icons, publisher, size, install date; search + sort; batch queue.
- **Native uninstall, then scan** — launches the software's own uninstaller (or `msiexec` for MSI), waits for it, then hunts leftovers. While an app is still installed, its live files are **locked** so you can't delete them by accident.
- **Force-remove mode** — no uninstaller registered, or the vendor one is broken? Deep-scan everything belonging to the app (program files + nested registry) and remove it manually.
- **Deep leftover scan** — files, registry, services, startup entries; hosts/tasks reported read-only. Windows system services are excluded by binary path (`C:\Windows`, `%SystemRoot%`, Defender's ProgramData home), not just name.
- **Confidence scoring** — every item scored High/Medium/Low from path matching, protected-path penalties, and fuzzy-name comparison.
- **Safe, reversible deletion**
  - Optional system **restore point** before deletion (default on)
  - Files → **soft trash**, restorable for 7 days
  - Registry subtrees → **backed up before removal**, restorable from History (kept 30 days)
  - Verify button re-scans and confirms what actually disappeared
  - Nothing deletes until you tick items and confirm
- **Reports** — JSON + TXT saved automatically to `%APPDATA%\ClearOut\reports` and listed in History.
- **AI advisory (opt-in)** — per-item analysis via Groq or OpenRouter free tiers. Sends only the item path/name; disabled by default, needs your own key.
- **Console themes** — JetBrains Mono UI, true-black dark or paper light, with Green / Cyan / Purple / Amber / Mono accent families.

## Download

Grab the latest installer from [Releases](https://github.com/rapzzzzz/clearout/releases).

> **Note:** builds are **unsigned** — Windows SmartScreen will warn on first launch. Click **More info → Run anyway**. Normal for free open-source software without a paid code-signing certificate.

**Run as administrator** for full power: leftover *service removal* and *restore points* need elevation (the app shows a banner when it isn't elevated).

## Screenshots

> Coming soon.

## Build from source

Prerequisites: [Rust (MSVC)](https://rustup.rs/), [Node.js](https://nodejs.org/) 20+, [VS Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) ("Desktop development with C++").

```bash
git clone https://github.com/rapzzzzz/clearout.git
cd clearout
npm install
npm run tauri dev      # development (hot reload)
npm run tauri build    # production → src-tauri/target/release/bundle/
```

## Privacy

- **Zero telemetry, zero analytics, zero ads.** The only time data leaves your machine is when you explicitly run the AI advisor with your own API key (sent to Groq/OpenRouter: the item path and name only — never file contents).
- Fonts are bundled locally — no startup network calls.
- Settings, reports (`%APPDATA%\ClearOut\reports`), soft trash and registry backups (`%LOCALAPPDATA%\ClearOut\`) all live on your machine — never on a server.

## Safety

- Protected system paths (`C:\Windows` and friends) are never deletable, whatever you select.
- Windows-owned services are filtered by binary path, and deletion refuses them defensively.
- Registry keys are snapshotted (raw values, lossless) before removal; restore lives in History.
- Files go to ClearOut's soft trash first, restorable from History for 7 days.
- Every deletion needs your explicit selection + confirmation dialog.

## Known limits (v0.1)

- Unsigned binary (SmartScreen).
- No auto-updater yet — install new releases from GitHub.
- Deep registry scan matches key *names*; value-level matching (CLSID `LocalServer32` etc.) is on the roadmap.
- Lock handling is a lightweight probe, not full Windows Restart Manager.

## Tech stack

| Layer | Tech |
|---|---|
| Backend | Rust, Tauri 2, winreg, windows |
| Frontend | Svelte 5, TypeScript, Tailwind CSS v4, Bits UI |
| Icons | Lucide |
| Font | JetBrains Mono (self-hosted) |

## License

[MIT](LICENSE) © 2026 rapzzzzz
