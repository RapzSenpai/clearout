use std::fs;
use std::path::PathBuf;
use chrono::Utc;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TrashEntry {
    pub id: String,
    pub original_path: String,
    pub trash_path: String,
    pub timestamp: String,
}

fn trash_dir() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(local).join("ClearOut").join("Trash")
    } else {
        PathBuf::from("ClearOutTrash")
    }
}

pub fn move_to_trash(original: &str) -> Result<String, String> {
    let src = std::path::Path::new(original);
    if !src.exists() {
        return Err(format!("Not found: {}", original));
    }
    let dir = trash_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let file_name = src.file_name().and_then(|n| n.to_str()).unwrap_or("item");
    let dest = dir.join(format!("{}_{}", id, file_name));

    // Try rename (same drive), fallback copy+remove for cross-drive
    let moved = fs::rename(src, &dest).is_ok() || {
        if src.is_dir() {
            copy_dir_all(src, &dest).is_ok() && fs::remove_dir_all(src).is_ok()
        } else {
            fs::copy(src, &dest).is_ok() && fs::remove_file(src).is_ok()
        }
    };
    if !moved {
        return Err(format!("Failed to move {} to trash", original));
    }

    // Write metadata sidecar. Suffix (not replace-extension) so a trashed
    // "settings.json" never gets overwritten by its own sidecar.
    let meta = PathBuf::from(format!("{}.json", dest.to_string_lossy()));
    let entry = TrashEntry {
        id: id.clone(),
        original_path: original.to_string(),
        trash_path: dest.to_string_lossy().to_string(),
        timestamp: Utc::now().to_rfc3339(),
    };
    let _ = fs::write(meta, serde_json::to_string_pretty(&entry).unwrap_or_default());

    // 7-day purge: remove entries older than 7 days
    purge_old(&dir);

    Ok(dest.to_string_lossy().to_string())
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let src_path = e.path();
        let dst_path = dst.join(e.file_name());
        if src_path.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn purge_old(dir: &PathBuf) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let now = Utc::now();
    for e in entries.filter_map(|x| x.ok()) {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(t) = fs::read_to_string(&p) {
            if let Ok(v) = serde_json::from_str::<TrashEntry>(&t) {
                if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&v.timestamp) {
                    let age = now.signed_duration_since(ts.with_timezone(&Utc));
                    if age.num_days() >= 7 {
                        let _ = fs::remove_file(&p);
                        let trash_path = PathBuf::from(v.trash_path);
                        if trash_path.exists() {
                            if trash_path.is_dir() { let _ = fs::remove_dir_all(trash_path); } else { let _ = fs::remove_file(trash_path); }
                        }
                    }
                }
            }
        }
    }
}

#[tauri::command]
pub fn list_trash() -> Result<Vec<TrashEntry>, String> {
    let dir = trash_dir();
    if !dir.exists() { return Ok(Vec::new()); }
    let mut out: Vec<TrashEntry> = Vec::new();
    for e in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(t) = fs::read_to_string(&p) {
                if let Ok(entry) = serde_json::from_str(&t) { out.push(entry); }
            }
        }
    }
    out.sort_by(|a,b| b.timestamp.cmp(&a.timestamp));
    Ok(out)
}

#[tauri::command]
pub fn restore_trash(id: String) -> Result<(), String> {    let dir = trash_dir();
    for e in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") { continue; }
        if let Ok(t) = fs::read_to_string(&p) {
            if let Ok(entry) = serde_json::from_str::<TrashEntry>(&t) {
                if entry.id == id {
                    let dest = std::path::Path::new(&entry.original_path);
                    if let Some(parent) = dest.parent() { let _ = fs::create_dir_all(parent); }
                    let src = PathBuf::from(&entry.trash_path);
                    if src.is_dir() {
                        copy_dir_all(&src, dest).map_err(|e| e.to_string())?;
                        let _ = fs::remove_dir_all(&src);
                    } else {
                        fs::copy(&src, dest).map_err(|e| e.to_string())?;
                        let _ = fs::remove_file(&src);
                    }
                    let _ = fs::remove_file(&p);
                    return Ok(())
                }
            }
        }
    }
    Err("Trash entry not found".to_string())
}

/// Permanently delete everything in the soft trash: the trashed files/folders
/// and their metadata sidecars. This is an early purge — the same outcome as
/// waiting out the 7-day TTL, but the files can no longer be restored.
#[tauri::command]
pub fn clear_trash() -> Result<u32, String> {
    let dir = trash_dir();
    if !dir.exists() {
        return Ok(0);
    }
    let entries = fs::read_dir(&dir).map_err(|e| e.to_string())?;
    let mut removed = 0u32;
    for e in entries.filter_map(|x| x.ok()) {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        if p.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(text) = fs::read_to_string(&p) {
                if let Ok(entry) = serde_json::from_str::<TrashEntry>(&text) {
                    // Sidecar: delete the trashed item it points to, then the sidecar.
                    let trash_path = PathBuf::from(&entry.trash_path);
                    // Guard: only delete inside the trash dir itself
                    if trash_path.starts_with(&dir) {
                        if trash_path.is_dir() {
                            let _ = fs::remove_dir_all(&trash_path);
                        } else if trash_path.is_file() {
                            let _ = fs::remove_file(&trash_path);
                        }
                    }
                    if fs::remove_file(&p).is_ok() {
                        removed += 1;
                    }
                    continue;
                }
            }
        }
        // Not a sidecar: it's a trashed item itself (e.g. a trashed .json file).
        if p.is_file() && fs::remove_file(&p).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}
