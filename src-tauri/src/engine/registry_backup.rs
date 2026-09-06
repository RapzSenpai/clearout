use base64::Engine;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use winreg::enums::*;
use winreg::enums::RegType;
use winreg::{RegKey, RegValue};

/// How long registry backups are kept before auto-purge.
const BACKUP_TTL_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueSnapshot {
    /// Value name; empty string = the key's default value.
    pub name: String,
    pub kind: String, // sz | expand | multi | dword | dword_be | qword | binary | link | none | unknown
    /// Raw registry bytes (exact copy — UTF-16 encoded for strings), base64.
    pub bytes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeySnapshot {
    /// Full path, e.g. HKEY_CURRENT_USER\SOFTWARE\Vendor\App
    pub path: String,
    pub values: Vec<ValueSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryBackup {
    pub id: String,
    pub created: String,
    pub root_path: String,
    pub keys: Vec<KeySnapshot>,
}

pub fn backup_dir() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(local).join("ClearOut").join("RegistryBackup")
    } else {
        PathBuf::from("ClearOutRegistryBackup")
    }
}

/// Splits "HKEY_CURRENT_USER\SOFTWARE\X" into (hive, relative path).
pub fn parse_hive(full_path: &str) -> Result<(RegKey, String), String> {
    let (hive_name, rest) = full_path
        .split_once('\\')
        .ok_or_else(|| format!("Invalid registry path: {}", full_path))?;
    let hive = match hive_name.to_uppercase().as_str() {
        "HKEY_CURRENT_USER" | "HKCU" => HKEY_CURRENT_USER,
        "HKEY_LOCAL_MACHINE" | "HKLM" => HKEY_LOCAL_MACHINE,
        _ => return Err(format!("Unsupported hive: {}", hive_name)),
    };
    Ok((RegKey::predef(hive), rest.to_string()))
}

fn kind_of(vtype: &RegType) -> String {
    match vtype {
        RegType::REG_SZ => "sz".to_string(),
        RegType::REG_EXPAND_SZ => "expand".to_string(),
        RegType::REG_MULTI_SZ => "multi".to_string(),
        RegType::REG_DWORD => "dword".to_string(),
        RegType::REG_DWORD_BIG_ENDIAN => "dword_be".to_string(),
        RegType::REG_QWORD => "qword".to_string(),
        RegType::REG_BINARY => "binary".to_string(),
        RegType::REG_LINK => "link".to_string(),
        RegType::REG_NONE => "none".to_string(),
        _ => "unknown".to_string(),
    }
}

fn snapshot_from_rv(name: &str, rv: &RegValue) -> ValueSnapshot {
    ValueSnapshot {
        name: name.to_string(),
        kind: kind_of(&rv.vtype),
        bytes: base64::engine::general_purpose::STANDARD.encode(&rv.bytes),
    }
}

/// Snapshot the key at `full_path` plus its whole subtree (all descendant keys).
pub fn capture_key_tree(full_path: &str) -> Result<Vec<KeySnapshot>, String> {
    let hive_prefix = full_path
        .split_once('\\')
        .map(|(h, _)| h.to_string())
        .unwrap_or_else(|| full_path.to_string());
    let (root_hive, rel_path) = parse_hive(full_path)?;

    let top = root_hive
        .open_subkey_with_flags(&rel_path, KEY_READ)
        .map_err(|e| format!("Failed to open {}: {}", rel_path, e))?;

    let mut out: Vec<KeySnapshot> = Vec::new();
    let mut stack: Vec<(RegKey, String)> = vec![(top, rel_path)];
    let mut guard: u32 = 0;

    while let Some((key, rel)) = stack.pop() {
        guard += 1;
        if guard > 200_000 {
            break;
        }
        let mut values = Vec::new();
        for (name, rv) in key.enum_values().filter_map(|v| v.ok()) {
            values.push(snapshot_from_rv(&name, &rv));
        }
        out.push(KeySnapshot {
            path: format!("{}\\{}", hive_prefix, rel),
            values,
        });

        let mut children: Vec<(String, RegKey)> = Vec::new();
        for sub in key.enum_keys().filter_map(|k| k.ok()) {
            if let Ok(child) = key.open_subkey_with_flags(&sub, KEY_READ) {
                children.push((sub, child));
            }
        }
        for (sub_name, child) in children.into_iter().rev() {
            stack.push((child, format!("{}\\{}", rel, sub_name)));
        }
    }

    Ok(out)
}

fn persist_backup(backup: &RegistryBackup) -> Result<String, String> {
    let dir = backup_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{}.json", backup.id));
    std::fs::write(&file, serde_json::to_string_pretty(backup).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Failed to write registry backup: {}", e))?;
    Ok(file.to_string_lossy().to_string())
}

/// Backs up a registry key tree before deletion. Returns the backup file path,
/// or Ok(None) when the key no longer exists (nothing to back up).
pub fn backup_registry_path(full_path: &str) -> Result<Option<String>, String> {
    let (_, rel_path) = parse_hive(full_path)?;
    let (root_hive, _) = parse_hive(full_path)?;
    if root_hive.open_subkey_with_flags(&rel_path, KEY_READ).is_err() {
        return Ok(None);
    }

    let keys = capture_key_tree(full_path)?;
    if keys.is_empty() {
        return Ok(None);
    }

    let backup = RegistryBackup {
        id: uuid::Uuid::new_v4().to_string(),
        created: Utc::now().to_rfc3339(),
        root_path: full_path.to_string(),
        keys,
    };
    let path = persist_backup(&backup)?;
    purge_old();
    Ok(Some(path))
}

/// Backs up a single value (e.g. a Startup Run entry) before deleting it.
pub fn backup_registry_value(key_full_path: &str, value_name: &str) -> Result<Option<String>, String> {
    let (root_hive, rel_path) = parse_hive(key_full_path)?;
    let key = match root_hive.open_subkey_with_flags(&rel_path, KEY_READ) {
        Ok(k) => k,
        Err(_) => return Ok(None),
    };
    let rv = match key.get_raw_value(value_name) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let backup = RegistryBackup {
        id: uuid::Uuid::new_v4().to_string(),
        created: Utc::now().to_rfc3339(),
        root_path: key_full_path.to_string(),
        keys: vec![KeySnapshot {
            path: key_full_path.to_string(),
            values: vec![snapshot_from_rv(value_name, &rv)],
        }],
    };
    let path = persist_backup(&backup)?;
    purge_old();
    Ok(Some(path))
}

fn load_backup(file: &std::path::Path) -> Option<RegistryBackup> {
    let text = std::fs::read_to_string(file).ok()?;
    serde_json::from_str::<RegistryBackup>(&text).ok()
}

/// Recreates backed-up keys/values. Keys are created shallow-first so
/// parents exist before children. Deletes the backup file on success.
pub fn restore_backup(id: &str) -> Result<(), String> {
    let dir = backup_dir();
    let file = dir.join(format!("{}.json", id));
    let backup = load_backup(&file)
        .ok_or_else(|| format!("Registry backup not found: {}", id))?;

    let mut ordered = backup.keys.clone();
    ordered.sort_by_key(|k| k.path.matches('\\').count());

    let mut errors: Vec<String> = Vec::new();
    for key in &ordered {
        let (root_hive, rel_path) = match parse_hive(&key.path) {
            Ok(v) => v,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let target = match create_key_path(&root_hive, &rel_path) {
            Ok(k) => k,
            Err(e) => {
                errors.push(format!("{}: {}", key.path, e));
                continue;
            }
        };
        for v in &key.values {
            let rv = RegValue {
                bytes: base64::engine::general_purpose::STANDARD
                    .decode(&v.bytes)
                    .unwrap_or_default(),
                vtype: type_of(&v.kind),
            };
            if let Err(e) = target.set_raw_value(&v.name, &rv) {
                errors.push(format!("{} <- {}: {}", key.path, v.name, e));
            }
        }
    }

    if errors.is_empty() {
        let _ = std::fs::remove_file(&file);
        Ok(())
    } else {
        Err(format!("Restore completed with errors:\n{}", errors.join("\n")))
    }
}

fn type_of(kind: &str) -> RegType {
    match kind {
        "sz" => RegType::REG_SZ,
        "expand" => RegType::REG_EXPAND_SZ,
        "multi" => RegType::REG_MULTI_SZ,
        "dword" => RegType::REG_DWORD,
        "dword_be" => RegType::REG_DWORD_BIG_ENDIAN,
        "qword" => RegType::REG_QWORD,
        "binary" => RegType::REG_BINARY,
        "link" => RegType::REG_LINK,
        "none" => RegType::REG_NONE,
        _ => RegType::REG_BINARY,
    }
}

fn create_key_path(root: &RegKey, rel_path: &str) -> Result<RegKey, String> {
    let mut current = root
        .open_subkey_with_flags("", KEY_WRITE)
        .map_err(|e| e.to_string())?;
    for seg in rel_path.split('\\') {
        if seg.is_empty() {
            continue;
        }
        current = match current.open_subkey_with_flags(seg, KEY_WRITE) {
            Ok(k) => k,
            Err(_) => current.create_subkey(seg).map_err(|e| e.to_string())?.0,
        };
    }
    Ok(current)
}

pub fn list_backups() -> Result<Vec<RegistryBackup>, String> {
    let dir = backup_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    purge_old();
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Some(b) = load_backup(&p) {
            out.push(b);
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(out)
}

pub fn delete_backup(id: &str) -> Result<(), String> {
    let file = backup_dir().join(format!("{}.json", id));
    if file.exists() {
        std::fs::remove_file(&file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Delete every backup JSON in the backup dir. Only touches *.json files
/// directly inside the dir.
pub fn clear_backups() -> Result<u32, String> {
    let dir = backup_dir();
    if !dir.exists() {
        return Ok(0);
    }
    let mut removed = 0u32;
    for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.is_file()
            && p.extension().and_then(|s| s.to_str()) == Some("json")
            && std::fs::remove_file(&p).is_ok()
        {
            removed += 1;
        }
    }
    Ok(removed)
}

fn purge_old() {
    let dir = backup_dir();
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let now = Utc::now();
    for e in entries.filter_map(|x| x.ok()) {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Some(b) = load_backup(&p) {
            if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&b.created) {
                let age = now.signed_duration_since(ts.with_timezone(&Utc));
                if age.num_days() >= BACKUP_TTL_DAYS {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hive_paths() {
        let (_, rel) = parse_hive(r"HKEY_CURRENT_USER\SOFTWARE\Vendor\App").unwrap();
        assert_eq!(rel, r"SOFTWARE\Vendor\App");
        assert!(parse_hive(r"HKEY_LOCAL_MACHINE\SOFTWARE").is_ok());
        assert!(parse_hive("HKEY_CLASSES_ROOT\\x").is_err());
        assert!(parse_hive("nohive\\x").is_err());
    }

    #[test]
    fn type_kind_roundtrip() {
        for kind in ["sz", "expand", "multi", "dword", "dword_be", "qword", "binary", "link", "none"] {
            assert_eq!(kind_of(&type_of(kind)), kind);
        }
    }

    #[test]
    fn snapshot_serde_roundtrip() {
        let vs = ValueSnapshot {
            name: "Path".into(),
            kind: "expand".into(),
            bytes: base64::engine::general_purpose::STANDARD.encode(b"%SystemRoot%\\x\\0\0"),
        };
        let json = serde_json::to_string(&vs).unwrap();
        let back: ValueSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "Path");
        assert_eq!(back.kind, "expand");
        assert_eq!(back.bytes, vs.bytes);
    }

    #[test]
    fn backup_struct_roundtrip() {
        let b = RegistryBackup {
            id: "abc".into(),
            created: "2026-01-01T00:00:00Z".into(),
            root_path: r"HKEY_CURRENT_USER\SOFTWARE\X".into(),
            keys: vec![KeySnapshot {
                path: r"HKEY_CURRENT_USER\SOFTWARE\X".into(),
                values: vec![ValueSnapshot {
                    name: "".into(),
                    kind: "sz".into(),
                    bytes: base64::engine::general_purpose::STANDARD.encode(b"x\x00"),
                }],
            }],
        };
        let json = serde_json::to_string(&b).unwrap();
        let back: RegistryBackup = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "abc");
        assert_eq!(back.keys.len(), 1);
        assert_eq!(back.keys[0].values[0].name, "");
    }
}
