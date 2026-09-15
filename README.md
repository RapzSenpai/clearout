# 🗑️ ClearOut

ClearOut uninstalls Windows applications and removes what they leave behind. It runs each application's own uninstaller, scans for leftover files, registry keys, services, and startup entries, and shows you every finding with a confidence rating before you approve the deletion.

**Free and open source (MIT). No ads or telemetry.**

> **Status:** v1.1.0 · Windows 10/11. Test it on disposable applications first.

## Download

Download the latest installer from [Releases](https://github.com/rapzzzzz/clearout/releases).

The binary carries no code signature, so SmartScreen shows a warning on first launch. Choose **More info → Run anyway**. The warning appears because the project has no paid code-signing certificate.

Run ClearOut as administrator for full functionality: service removal and restore points require elevation. The application shows a banner when it runs without elevation.

## Screenshots

![Dashboard](screenshots/dashboard.png)
![Leftover review](screenshots/leftoverreview.png)
![Settings](screenshots/settings.png)

## Features

- **Application inventory**: ClearOut reads the Windows registry and lists installed applications with their icons, publisher, size, and install date. You can search, sort, and queue several uninstalls at once.
- **Native uninstall, then scan**: the application's own uninstaller runs first, and `msiexec` handles MSI packages. After it finishes, ClearOut scans for what remains. Files of applications still installed stay locked, so you cannot delete them by mistake.
- **No-uninstaller mode**: use "App already gone? Scan leftovers instead" when an uninstaller is missing or broken. ClearOut finds everything belonging to the application and removes it after your confirmation.
- **Leftover scan**: ClearOut checks files, registry keys, services, and startup entries. It identifies Windows-owned services by their binary path, so a matching name alone cannot flag a system service.
- **Confidence scoring**: each finding receives a High, Medium, or Low rating before you approve anything.
- **Destructive deletion**: ClearOut can create a system restore point before deleting (enabled by default). Files and folders are deleted permanently — locked files are skipped or scheduled for removal after restart, never forced. Registry subtrees are backed up before removal and stay restorable for 30 days. After deletion, Run another scan re-scans and reports what disappeared. ClearOut deletes nothing until you select items and confirm.
- **Reports**: ClearOut writes JSON and TXT reports to `%APPDATA%\ClearOut\reports` and lists them in History.
- **AI advisory (opt-in)**: with your own API key from Groq, OpenRouter, OpenAI, or DeepSeek — or a local Ollama model, or any OpenAI-compatible custom endpoint — an AI model can assess individual findings. Test the connection from Settings before you review. Requests contain the item path and name only. The feature is off by default.
- **Console interface**: JetBrains Mono typography, dark and light modes, and five accent colors.

## Safety

- **Safety**: ClearOut refuses to delete protected system paths on any drive (`C:\Windows`, drive roots, `Program Files` / `Users` roots), regardless of your selection.
- It identifies Windows-owned services by binary path and checks the path again before deleting a service.
- ClearOut snapshots registry keys with their raw values before removal, so you can restore them from History.
- File deletion is permanent. Locked files are skipped independently so one lock never fails the whole operation.
- Every deletion requires your explicit selection plus a confirmation dialog.

## Privacy

- The application contains no telemetry or analytics and contacts no server at startup.
- The AI advisor is the only feature that uses the network, and it does so only after you enable it with your own API key. Requests carry the item path and name; file contents never leave your machine.
- Settings, reports, and registry backups stay in your user folders.

## Known limitations (v1.1.0)

- The binary is unsigned, so SmartScreen warns on first launch.
- There is no auto-updater; install new versions from GitHub.
- The registry scan matches key names. Value-level matching (for example, CLSID `LocalServer32`) is on the roadmap.
- File locking uses a lightweight probe rather than the full Windows Restart Manager.
- File deletion is permanent (no recycle bin). Registry deletions are backed up for 30 days; files rely on restore points.
- API keys live in Windows Credential Manager per provider. Custom AI endpoints require https except localhost.

## Checks

```bash
npm run check      # svelte-check + tsc, must report 0 errors
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib   # Rust unit tests
cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings
```

## Build from source

You need [Rust (MSVC)](https://rustup.rs/), [Node.js](https://nodejs.org/) 20 or later, and [VS Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the "Desktop development with C++" workload.

```bash
git clone https://github.com/rapzzzzz/clearout.git
cd clearout
npm install
npm run tauri dev    # development (hot reload)
npm run tauri build  # installer → src-tauri/target/release/bundle/
```

## License

[MIT](LICENSE) © 2026 rapzzzzz
