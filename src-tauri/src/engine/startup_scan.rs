use crate::engine::matching::{match_name, match_name_strict, AppSignals, MatchKind};
use crate::models::{AppInfo, LeftoverItem, LeftoverType};
use std::path::Path;
use winreg::enums::*;
use winreg::RegKey;

pub fn scan_startup(app: &AppInfo) -> Result<Vec<LeftoverItem>, String> {
    let mut items: Vec<LeftoverItem> = Vec::new();
    let sig = AppSignals::from_app(app);

    let reg_keys = vec![
        (HKEY_CURRENT_USER, "HKEY_CURRENT_USER", "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run"),
        (HKEY_CURRENT_USER, "HKEY_CURRENT_USER", "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\RunOnce"),
        (HKEY_LOCAL_MACHINE, "HKEY_LOCAL_MACHINE", "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run"),
        (HKEY_LOCAL_MACHINE, "HKEY_LOCAL_MACHINE", "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\RunOnce"),
        (HKEY_LOCAL_MACHINE, "HKEY_LOCAL_MACHINE", "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run"),
        (HKEY_LOCAL_MACHINE, "HKEY_LOCAL_MACHINE", "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\RunOnce"),
    ];

    for (hive, hive_name, key_path) in reg_keys {
        let reg_key = RegKey::predef(hive);
        if let Ok(key) = reg_key.open_subkey_with_flags(key_path, KEY_READ) {
            for (value_name, value_data) in key.enum_values().filter_map(|v| v.ok()) {
                let name_lower = value_name.to_lowercase();
                let data_str = value_data.to_string().to_lowercase();

                // Strict on both value name and command line: a lone keyword
                // must not flag arbitrary Run entries (they execute on login).
                let matched = match_name_strict(&sig, &name_lower) != MatchKind::None
                    || match_name_strict(&sig, &data_str) != MatchKind::None;

                if matched {
                    let full_path = format!("{}\\{} -> {}", hive_name, key_path, value_name);
                    items.push(LeftoverItem {
                        id: crate::models::make_leftover_id(&LeftoverType::Startup, &full_path),
                        path: full_path,
                        item_type: LeftoverType::Startup,
                        confidence: 0,
                        confidence_tier: crate::models::ConfidenceTier::Low,
                        associated_app: app.name.clone(),
                        size: None,
                    });
                }
            }
        }
    }

    // Check Startup folders
    let mut startup_folders = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        startup_folders.push(format!("{}\\Microsoft\\Windows\\Start Menu\\Programs\\Startup", appdata));
    }
    if let Ok(programdata) = std::env::var("PROGRAMDATA") {
        startup_folders.push(format!("{}\\Microsoft\\Windows\\Start Menu\\Programs\\Startup", programdata));
    }

    for folder in startup_folders {
        let p = Path::new(&folder);
        if p.exists() && p.is_dir() {
            if let Ok(entries) = std::fs::read_dir(p) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    let file_name_lower = file_name.to_lowercase();

                    if match_name(&sig, &file_name_lower) == MatchKind::None {
                        continue;
                    }

                    let full_path = entry.path().to_string_lossy().to_string();
                    items.push(LeftoverItem {
                        id: crate::models::make_leftover_id(&LeftoverType::Startup, &full_path),
                        path: full_path,
                        item_type: LeftoverType::Startup,
                        confidence: 0,
                        confidence_tier: crate::models::ConfidenceTier::Low,
                        associated_app: app.name.clone(),
                        size: entry.metadata().ok().map(|m| m.len()),
                    });
                }
            }
        }
    }

    Ok(items)
}

