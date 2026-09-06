use std::path::Path;

#[tauri::command]
pub fn open_location(path: String) -> Result<(), String> {
    // Reject hosts pseudo-paths and empty
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Empty path".to_string());
    }
    if trimmed.starts_with("hosts:") {
        return Err("Hosts entry is read-only — open C:\\Windows\\System32\\drivers\\etc\\hosts manually".to_string());
    }

    let p = Path::new(trimmed);
    // If file exists, reveal parent in explorer and select it
    // If not exists, try to open parent directory
    let target = if p.exists() {
        p.to_path_buf()
    } else {
        // Maybe registry path — not openable
        if trimmed.starts_with("HKEY_") || trimmed.contains(" -> ") || trimmed.starts_with("Service: ") {
            return Err("Not a file path — cannot open location".to_string());
        }
        // Try parent of non-existing leftover
        if let Some(parent) = p.parent() {
            if parent.exists() {
                parent.to_path_buf()
            } else {
                return Err(format!("Path does not exist: {}", trimmed));
            }
        } else {
            return Err(format!("Invalid path: {}", trimmed));
        }
    };

    // Use explorer /select for files, plain explorer for folders
    let is_file = target.is_file();
    let result = if is_file {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(target.to_string_lossy().to_string())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open explorer: {}", e))
    } else {
        std::process::Command::new("explorer")
            .arg(target.to_string_lossy().to_string())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open explorer: {}", e))
    };
    result
}
