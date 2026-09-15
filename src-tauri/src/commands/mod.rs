pub mod inventory;
pub mod uninstall;
pub mod scan;
pub mod delete;
pub mod ai;
pub mod export;
pub mod open_location;
pub mod history;
pub mod scheduler;
pub mod registry_backup;
pub mod secure_storage;

use std::process::Command;

/// Spawn a console program (schtasks, sc.exe, powershell, tasklist) with
/// CREATE_NO_WINDOW so no CMD window flashes while it runs. GUI apps
/// (vendor uninstallers, explorer, msiexec dialogs) are unaffected — they
/// own their windows.
#[cfg(windows)]
pub(crate) fn silent(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new(program);
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

#[cfg(not(windows))]
pub(crate) fn silent(program: &str) -> Command {
    Command::new(program)
}

/// ponytail: file log beats log framework; ceiling is 512KB single-file rotation, upgrade is structured logger.
pub(crate) fn log_event(level: &str, msg: &str) {
    let dir = std::env::var_os("APPDATA")
        .map(|a| std::path::PathBuf::from(a).join("ClearOut").join("logs"));
    let Some(dir) = dir else { return };
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("app.log");
    // Rotate once past 512KB; keep one backup. Cheap, no dependency.
    if let Ok(md) = std::fs::metadata(&file) {
        if md.len() > 512 * 1024 {
            let _ = std::fs::remove_file(dir.join("app.log.1"));
            let _ = std::fs::rename(&file, dir.join("app.log.1"));
        }
    }
    let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let line = format!("[{}] {}: {}\n", ts, level, msg.chars().take(2000).collect::<String>());
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// ponytail: tail-read beats log viewer; ceiling is last 200KB, upgrade is filtered query.
#[tauri::command]
pub fn read_app_logs(limit_bytes: Option<u64>) -> Result<String, String> {
    let Some(appdata) = std::env::var_os("APPDATA") else {
        return Err("APPDATA missing".to_string());
    };
    let file = std::path::PathBuf::from(appdata).join("ClearOut").join("logs").join("app.log");
    let data = std::fs::read(&file).map_err(|e| e.to_string())?;
    let cap = limit_bytes.unwrap_or(200 * 1024).min(data.len() as u64) as usize;
    let tail = &data[data.len() - cap..];
    Ok(String::from_utf8_lossy(tail).to_string())
}
