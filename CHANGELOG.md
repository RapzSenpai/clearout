# Changelog

## v1.1.0

Release blockers fixed, safety architecture preserved.

- Versions synchronized to 1.1.0 across `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.
- Removed duplicate `ask_ai` / `test_ai_connection` handler registrations.
- Permanent delete now warns clearly, requires typed `DELETE` for 20+ items, and calls out reboot-scheduled files that persist after quit.
- Restore-point failure blocks deletion unless explicitly overridden.
- Registry delete shows live subtree impact (keys + values) and still requires explicit leaf consent plus mandatory backup.
- Uninstall now previews full command, publisher, and MSI flag before execution.
- Path traversal hardened: report read/delete uses filename-only canonical resolution; registry backup ids use allowlist; `APPDATA` / `LOCALAPPDATA` missing fails loudly instead of falling back to working directory.
- API keys moved out of `localStorage` into Windows Credential Manager; custom endpoints require `https` except localhost; AI requests carry a 15-second timeout.
- Production logging added to `%APPDATA%\ClearOut\logs\app.log` with 512KB rotation plus `read_app_logs` tail command.
- `LeftoverReview` split: `UninstallPreviewList`, `RegistryImpactList`, `review-helpers`; review/dashboard/history tables paginated (100 / 100 / 50); dashboard icon cache keeps icons across refreshes.
- Dependencies trimmed: `tokio` full replaced by `rt-multi-thread` + `macros`; CSP drops `ws://localhost`.
- Tests added for traversal, backup-id allowlist, HTTPS rule, protected paths/services, and ID stability.

Safety preserved: protected paths, protected services (fail closed), mandatory registry/startup backup, locked-file protection, double-delete guard.
