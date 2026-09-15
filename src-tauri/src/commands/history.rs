use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    pub path: String,
    pub filename: String,
    pub app_name: String,
    pub timestamp: String,
    pub size: u64,
}

fn get_reports_dir() -> Result<PathBuf, String> {
    // ponytail: fail loudly beats dot-fallback; ceiling is hard error when APPDATA missing, upgrade is configurable data dir.
    if let Some(appdata) = std::env::var_os("APPDATA") {
        Ok(PathBuf::from(appdata).join("ClearOut").join("reports"))
    } else {
        Err("APPDATA missing: cannot resolve reports directory".to_string())
    }
}

fn resolve_inside_reports(reports_dir: &std::path::Path, raw: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(raw);
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("Invalid report path".to_string());
    }
    let base = reports_dir
        .canonicalize()
        .unwrap_or_else(|_| reports_dir.to_path_buf());
    let joined = base.join(p.file_name().ok_or_else(|| "Invalid report path".to_string())?);
    let canon = joined.canonicalize().unwrap_or_else(|_| joined.clone());
    if !canon.starts_with(&base) {
        return Err("Invalid report path".to_string());
    }
    match canon.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()) {
        Some(e) if e == "json" || e == "txt" => Ok(canon),
        _ => Err("Invalid report path".to_string()),
    }
}

#[tauri::command]
pub fn list_reports() -> Result<Vec<HistoryEntry>, String> {
    let dir = get_reports_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(&dir).map_err(|e| e.to_string())?;
    let mut out: Vec<HistoryEntry> = Vec::new();
    for e in entries.filter_map(|x| x.ok()) {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let meta = e.metadata().map_err(|e| e.to_string())?;
        let filename = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        // Parse app_name from export JSON if possible
        let app_name = fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("app_name").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| filename.clone());
        let timestamp = fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("timestamp").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .unwrap_or_default();
        out.push(HistoryEntry {
            path: p.to_string_lossy().to_string(),
            filename,
            app_name,
            timestamp,
            size: meta.len(),
        });
    }
    out.sort_by(|a, b| b.filename.cmp(&a.filename));
    Ok(out)
}

#[tauri::command]
pub fn load_report(path: String) -> Result<String, String> {
    // Guard: only allow reading inside reports dir
    let reports_dir = get_reports_dir()?;
    let p = resolve_inside_reports(&reports_dir, &path)?;
    fs::read_to_string(&p).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_report(path: String) -> Result<(), String> {
    let reports_dir = get_reports_dir()?;
    let p = resolve_inside_reports(&reports_dir, &path)?;
    // Also try to delete adjacent .txt if exists
    if p.exists() {
        fs::remove_file(&p).map_err(|e| e.to_string())?;
    }
    let txt_path = p.with_extension("txt");
    if txt_path.exists() {
        let _ = fs::remove_file(txt_path);
    }
    Ok(())
}

/// Delete every report (JSON + matching TXT) in the reports dir.
/// Only touches files directly inside the reports dir — nothing else.
#[tauri::command]
pub fn clear_reports() -> Result<u32, String> {
    let dir = get_reports_dir()?;
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
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ext != "json" && ext != "txt" {
            continue;
        }
        if fs::remove_file(&p).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_traversal() {
        let base = PathBuf::from(r"C:\Users\T\AppData\Roaming\ClearOut\reports");
        assert!(resolve_inside_reports(&base, r"..\Windows\evil.json").is_err());
        // Absolute input collapses to bare filename inside base — safe, not traversal.
        let collapsed = resolve_inside_reports(&base, r"C:\Windows\evil.json").unwrap();
        assert!(collapsed.starts_with(&base));
        assert!(resolve_inside_reports(&base, "note.exe").is_err());
    }

    #[test]
    fn accepts_bare_filename_only() {
        let base = PathBuf::from(r"C:\Users\T\AppData\Roaming\ClearOut\reports");
        let ok = resolve_inside_reports(&base, "clearout-app-20260101.json").unwrap();
        assert!(ok.starts_with(&base));
    }
}
