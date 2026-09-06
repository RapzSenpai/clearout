use crate::models::{DeleteResult, LeftoverItem, LeftoverType};
use std::path::Path;
use tauri::{AppHandle, Emitter};
use winreg::enums::*;
use winreg::RegKey;

#[tauri::command]
pub async fn delete_items(app: AppHandle, items: Vec<LeftoverItem>, create_restore_point: bool, force_kill_allowed: Option<bool>) -> Result<DeleteResult, String> {
    let _ = app.emit("delete-progress", serde_json::json!({"stage": "start", "current": 0, "total": items.len(), "percent": 0}));

    let result = tokio::task::spawn_blocking(move || {
        let mut deleted = 0;
        let mut skipped = 0;
        let mut skipped_items: Vec<crate::models::SkippedItem> = Vec::new();
        let mut already_gone = 0usize;
        let mut already_gone_paths: Vec<String> = Vec::new();
        let mut trashed: Vec<String> = Vec::new();
        let mut deleted_ids: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        // Restore point is tracked separately from item failures: a skipped
        // checkpoint (e.g. not elevated, System Protection off) is not a
        // failed deletion and must never read as one in the UI.
        let mut restore_point_ok = false;
        let mut restore_point_error: Option<String> = None;
        let total = items.len();

        // Records a skip with the exact rule that caused it.
        let mut skip = |item: &LeftoverItem, reason: &str| {
            skipped += 1;
            skipped_items.push(crate::models::SkippedItem {
                path: item.path.clone(),
                reason: reason.to_string(),
            });
        };

        if create_restore_point {
            let _ = app.emit("delete-progress", serde_json::json!({"stage": "restore-point", "current": 0, "total": total, "percent": 5}));
            match create_system_restore_point() {
                Ok(_) => restore_point_ok = true,
                Err(e) => restore_point_error = Some(e),
            }
        }

        for (idx, item) in items.iter().enumerate() {
            let current = idx + 1;
            let percent = if total == 0 { 100 } else { 5 + (current as f64 / total as f64 * 90.0) as u64 };
            let stage = match item.item_type {
                LeftoverType::File | LeftoverType::Folder => "files",
                LeftoverType::Registry => "registry",
                LeftoverType::Startup => "startup",
                LeftoverType::Service => "services",
                LeftoverType::Hosts => "hosts",
                LeftoverType::Task => "tasks",
            };
            let _ = app.emit("delete-progress", serde_json::json!({
                "stage": stage,
                "current": current,
                "total": total,
                "percent": percent,
                "path": item.path
            }));

            match item.item_type {
                LeftoverType::File | LeftoverType::Folder => {
                    let path = Path::new(&item.path);
                    if is_protected_path(&item.path) {
                        skip(item, "Protected Windows location");
                        continue;
                    }
                    if path.exists() {
                        match crate::commands::trash::move_to_trash(&item.path) {
                            Ok(_) => {
                                deleted += 1;
                                trashed.push(item.path.clone());
                                deleted_ids.push(item.id.clone());
                            }
                            Err(e) => errors.push(format!("Trash failed {}: {}", item.path, e)),
                        }
                    } else {
                        already_gone += 1;
                        already_gone_paths.push(item.path.clone());
                    }
                }
                LeftoverType::Registry => {
                    if is_protected_registry(&item.path) {
                        skip(item, "Protected Windows registry key");
                        continue;
                    }

                    // Reversible deletions: snapshot the key tree first. If the
                    // backup cannot be written, refuse to delete.
                    match crate::engine::registry_backup::backup_registry_path(&item.path) {
                        Ok(_) => match delete_registry_entry(&item.path) {
                            Ok(_) => { deleted += 1; deleted_ids.push(item.id.clone()); }
                            Err(e) => errors.push(format!("Failed to delete registry {}: {}", item.path, e)),
                        },
                        Err(e) => errors.push(format!(
                            "Registry backup failed — delete skipped for {}: {}",
                            item.path, e
                        )),
                    }
                }
                LeftoverType::Startup => {
                    let path_str = &item.path;
                    if path_str.contains(" -> ") {
                        let parts: Vec<&str> = path_str.split(" -> ").collect();
                        let backup_ok = if parts.len() == 2 {
                            crate::engine::registry_backup::backup_registry_value(parts[0], parts[1])
                        } else {
                            Ok(None)
                        };
                        match backup_ok {
                            Ok(_) => match delete_startup_registry(path_str) {
                                Ok(_) => { deleted += 1; deleted_ids.push(item.id.clone()); }
                                Err(e) => errors.push(format!("Failed to delete startup entry {}: {}", path_str, e)),
                            },
                            Err(e) => errors.push(format!(
                                "Registry backup failed — startup delete skipped for {}: {}",
                                path_str, e
                            )),
                        }
                    } else {
                        let path = Path::new(path_str);
                        if path.exists() && path.is_file() {
                            match crate::commands::trash::move_to_trash(path_str) {
                                Ok(_) => { deleted += 1; trashed.push(item.path.clone()); deleted_ids.push(item.id.clone()); }
                                Err(e) => errors.push(format!("Trash failed {}: {}", path_str, e)),
                            }
                        } else {
                            already_gone += 1;
                            already_gone_paths.push(item.path.clone());
                        }
                    }
                }
                LeftoverType::Service => {
                    let service_name = if let Some(rest) = item.path.strip_prefix("Service: ") {
                        if let Some(paren_idx) = rest.find(" (") {
                            &rest[..paren_idx]
                        } else {
                            rest
                        }
                    } else {
                        &item.path
                    };

                    // Defense-in-depth: the scan filters system services, but a
                    // stale pending cleanup could still carry one. Never touch a
                    // service whose binary lives under C:\Windows.
                    if is_protected_service(service_name) {
                        skip(item, "Windows-owned service");
                        continue;
                    }

                    match delete_windows_service(service_name, force_kill_allowed.unwrap_or(false)) {
                        Ok(_) => { deleted += 1; deleted_ids.push(item.id.clone()); }
                        Err(e) => errors.push(format!("Failed to delete service {}: {}", service_name, e)),
                    }
                }
                LeftoverType::Hosts | LeftoverType::Task => {
                    // Read-only detection only — never auto-delete System32/hosts or Task files
                    let what = if matches!(item.item_type, LeftoverType::Hosts) {
                        "Hosts entry — ClearOut doesn't edit the hosts file"
                    } else {
                        "Scheduled task — ClearOut doesn't remove scheduled tasks"
                    };
                    skip(item, what);
                }
            }
        }

        let _ = app.emit("delete-progress", serde_json::json!({"stage": "done", "current": total, "total": total, "percent": 100}));

        DeleteResult {
            deleted,
            skipped,
            skipped_items,
            already_gone,
            already_gone_paths,
            trashed,
            deleted_ids,
            errors,
            restore_point_ok,
            restore_point_error,
        }
    }).await.map_err(|e| format!("Delete task failed: {}", e))?;

    Ok(result)
}

fn is_protected_path(path: &str) -> bool {
    // Only block true system dirs — matches confidence.rs + blueprint 5.2
    // Do NOT block Program Files / ProgramData / Users — leftovers live there and must be deletable
    let protected = vec![
        "C:\\Windows",
        "C:\\Windows\\System32",
        "C:\\Windows\\SysWOW64",
        "C:\\Windows\\WinSxS",
        "C:\\Windows\\Fonts",
        "C:\\Windows\\Boot",
        "C:\\Windows\\Installer",
        "C:\\Windows\\Prefetch",
        "C:\\Windows\\Temp",
    ];
    let lower = path.to_lowercase();
    for p in protected {
        let p_lower = p.to_lowercase();
        if lower == p_lower || lower.starts_with(&format!("{}\\", p_lower)) {
            return true;
        }
    }
    false
}

fn is_protected_registry(path: &str) -> bool {
    let lower = path.to_lowercase();
    let parts: Vec<&str> = lower.split('\\').collect();
    if parts.len() <= 2 {
        return true;
    }
    let critical = [
        "hkey_local_machine\\software",
        "hkey_current_user\\software",
        "hkey_local_machine\\software\\microsoft",
        "hkey_current_user\\software\\microsoft",
        "hkey_local_machine\\software\\microsoft\\windows",
        "hkey_current_user\\software\\microsoft\\windows",
        "hkey_local_machine\\software\\microsoft\\windows\\currentversion",
        "hkey_current_user\\software\\microsoft\\windows\\currentversion",
        "hkey_local_machine\\software\\microsoft\\windows\\currentversion\\uninstall",
        "hkey_current_user\\software\\microsoft\\windows\\currentversion\\uninstall",
    ];
    for c in critical {
        if lower == c {
            return true;
        }
    }
    false
}

fn delete_registry_entry(path: &str) -> Result<(), String> {
    let parts: Vec<&str> = path.splitn(2, '\\').collect();
    if parts.len() < 2 {
        return Err("Invalid registry path".to_string());
    }

    let (hive_name, subpath) = (parts[0], parts[1]);
    let hive = match hive_name {
        "HKEY_CURRENT_USER" => HKEY_CURRENT_USER,
        "HKEY_LOCAL_MACHINE" => HKEY_LOCAL_MACHINE,
        _ => return Err(format!("Unsupported hive: {}", hive_name)),
    };

    let last_backslash = subpath.rfind('\\').ok_or_else(|| "Invalid subkey path".to_string())?;
    let parent_path = &subpath[..last_backslash];
    let key_to_delete = &subpath[last_backslash + 1..];

    let root = RegKey::predef(hive);
    let parent_key = root.open_subkey_with_flags(parent_path, KEY_WRITE)
        .map_err(|e| format!("Failed to open parent key {}: {}", parent_path, e))?;

    parent_key.delete_subkey_all(key_to_delete)
        .map_err(|e| format!("Failed to delete subkey {}: {}", key_to_delete, e))?;

    Ok(())
}

fn delete_startup_registry(entry: &str) -> Result<(), String> {
    // Format: "HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\Run -> ValueName"
    let parts: Vec<&str> = entry.split(" -> ").collect();
    if parts.len() != 2 {
        return Err("Invalid startup entry format".to_string());
    }

    let key_full = parts[0];
    let value_name = parts[1];

    let key_parts: Vec<&str> = key_full.splitn(2, '\\').collect();
    if key_parts.len() < 2 {
        return Err("Invalid key path".to_string());
    }

    let hive = match key_parts[0] {
        "HKEY_CURRENT_USER" => HKEY_CURRENT_USER,
        "HKEY_LOCAL_MACHINE" => HKEY_LOCAL_MACHINE,
        _ => return Err("Unsupported hive".to_string()),
    };

    let root = RegKey::predef(hive);
    let key = root.open_subkey_with_flags(key_parts[1], KEY_WRITE)
        .map_err(|e| format!("Failed to open key {}: {}", key_parts[1], e))?;

    key.delete_value(value_name)
        .map_err(|e| format!("Failed to delete startup value {}: {}", value_name, e))?;

    Ok(())
}

fn delete_windows_service(service_name: &str, force_kill: bool) -> Result<(), String> {
    // Try stopping service
    let stop_output = crate::commands::silent("sc.exe")
        .args(["stop", service_name])
        .output();

    if let Ok(out) = &stop_output {
        if !out.status.success() && !force_kill {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
            return Err(format!("Service stop denied (force_kill_allowed is off): {}", detail));
        }
    }

    // Delete service
    let output = crate::commands::silent("sc.exe")
        .args(["delete", service_name])
        .output()
        .map_err(|e| format!("Failed to run sc delete: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        // sc.exe writes its failure text to stdout, not stderr.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
        Err(format!("Service delete failed: {}", detail))
    }
}

/// True when the service is Windows-owned: its ImagePath lives under
/// C:\Windows. Falls back to a known-name blocklist if the registry read fails.
fn is_protected_service(service_name: &str) -> bool {
    if let Some(image) = read_service_image_path(service_name) {
        let normalized = crate::engine::service_scan::normalized_image_path(&image);
        if !normalized.is_empty() {
            return crate::engine::service_scan::is_system_service_path(&normalized);
        }
    }
    const SYSTEM_NAMES: &[&str] = &[
        "dhcp", "coremessagingregistrar", "msseccore", "winmgmt", "spooler", "wuauserv",
        "bits", "cryptsvc", "dcomlaunch", "rpcss", "lmhosts", "dnscache", "eventlog",
        "themes", "audiosrv", "wscsvc", "bthserv", "fontcache", "storsvc", "sysmain",
        "winsock", "mpssvc", "bfe", "basessvc", "brokerinfrastructure",
    ];
    SYSTEM_NAMES.iter().any(|s| service_name.eq_ignore_ascii_case(s))
}

fn read_service_image_path(service_name: &str) -> Option<String> {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            format!("SYSTEM\\CurrentControlSet\\Services\\{}", service_name),
            KEY_READ,
        )
        .ok()?;
    key.get_value("ImagePath").ok()
}

fn create_system_restore_point() -> Result<(), String> {
    let output = crate::commands::silent("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "Checkpoint-Computer -Description 'ClearOut Restore Point' -RestorePointType 'MODIFY_SETTINGS'"
        ])
        .output()
        .map_err(|e| format!("Failed to run PowerShell: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
        let lower = detail.to_lowercase();
        let hint = if lower.contains("access") || lower.contains("denied") || lower.contains("elevated") {
            " — restore points need to run as Administrator and System Protection enabled for the drive"
        } else if lower.contains("system restore") || lower.contains("disabled") {
            " — enable System Protection for this drive in System Properties → System Protection"
        } else {
            ""
        };
        Err(format!("{}{}", detail, hint))
    }
}
