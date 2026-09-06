use crate::engine::matching::{match_name, AppSignals, MatchKind};
use crate::models::{AppInfo, LeftoverItem, LeftoverType};
use std::collections::HashSet;
use winreg::enums::*;
use winreg::RegKey;

pub fn scan_registry(app: &AppInfo, scan_depth: &str) -> Result<Vec<LeftoverItem>, String> {
    let mut items: Vec<LeftoverItem> = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();

    let sig = AppSignals::from_app(app);
    let publisher_lower = sig.publisher_lower.clone();

    let targets = vec![
        (HKEY_CURRENT_USER, "HKEY_CURRENT_USER", "SOFTWARE"),
        (HKEY_LOCAL_MACHINE, "HKEY_LOCAL_MACHINE", "SOFTWARE"),
        (HKEY_LOCAL_MACHINE, "HKEY_LOCAL_MACHINE", "SOFTWARE\\WOW6432Node"),
        (HKEY_CURRENT_USER, "HKEY_CURRENT_USER", "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (HKEY_LOCAL_MACHINE, "HKEY_LOCAL_MACHINE", "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (HKEY_LOCAL_MACHINE, "HKEY_LOCAL_MACHINE", "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (HKEY_CURRENT_USER, "HKEY_CURRENT_USER", "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths"),
        (HKEY_LOCAL_MACHINE, "HKEY_LOCAL_MACHINE", "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths"),
    ];

    let system_skip = [
        "microsoft", "windows", "classes", "policies", "systemcert", "registeredapplications",
        "oem", "clients", "program groups",
    ];

    for (hive, hive_name, base_path) in targets {
        let root = RegKey::predef(hive);
        let key = match root.open_subkey_with_flags(base_path, KEY_READ) {
            Ok(k) => k,
            Err(_) => continue,
        };

        for subkey_name in key.enum_keys().filter_map(|k| k.ok()) {
            let subkey_lower = subkey_name.to_lowercase();
            let is_root_software = base_path == "SOFTWARE" || base_path == "SOFTWARE\\WOW6432Node";

            let matched = match_name(&sig, &subkey_lower);

            if matched != MatchKind::None {
                // Safety check: don't match critical system keys
                if is_root_software && system_skip.iter().any(|s| &subkey_lower == s) {
                    // Skip root system key
                } else {
                    let full_path = format!("{}\\{}\\{}", hive_name, base_path, subkey_name);
                    if seen_paths.insert(full_path.clone()) {
                        items.push(LeftoverItem {
                        id: crate::models::make_leftover_id(&LeftoverType::Registry, &full_path),
                        path: full_path,
                            item_type: LeftoverType::Registry,
                            confidence: 0,
                            confidence_tier: crate::models::ConfidenceTier::Low,
                            associated_app: app.name.clone(),
                            size: None,
                        });
                    }
                }
            } else if is_root_software {
                // Check if this subkey is a vendor / publisher folder (e.g. Google, Adobe, Steam)
                let is_publisher = (!publisher_lower.is_empty() && subkey_lower.contains(&publisher_lower))
                    || sig.distinctive.contains(&subkey_lower);

                if is_publisher && !system_skip.iter().any(|s| &subkey_lower == s) {
                    if let Ok(vendor_key) = key.open_subkey_with_flags(&subkey_name, KEY_READ) {
                        for child_name in vendor_key.enum_keys().filter_map(|k| k.ok()) {
                            let child_lower = child_name.to_lowercase();

                            if match_name(&sig, &child_lower) != MatchKind::None {
                                let full_path = format!("{}\\{}\\{}\\{}", hive_name, base_path, subkey_name, child_name);
                                if seen_paths.insert(full_path.clone()) {
                                    items.push(LeftoverItem {
                                        id: crate::models::make_leftover_id(&LeftoverType::Registry, &full_path),
                                        path: full_path,
                                        item_type: LeftoverType::Registry,
                                        confidence: 0,
                                        confidence_tier: crate::models::ConfidenceTier::Low,
                                        associated_app: app.name.clone(),
                                        size: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // "force" mode: crawl a few levels deeper inside each already-matched key and
    // add descendant keys whose names still reference the app.
    if scan_depth == "force" {
        let registry_roots: Vec<String> = items
            .iter()
            .filter(|i| matches!(i.item_type, LeftoverType::Registry))
            .map(|i| i.path.clone())
            .collect();
        for full_root in registry_roots {
            if let Some((hive_name, rel)) = full_root.split_once('\\') {
                let hive = match hive_name.to_uppercase().as_str() {
                    "HKEY_CURRENT_USER" => HKEY_CURRENT_USER,
                    "HKEY_LOCAL_MACHINE" => HKEY_LOCAL_MACHINE,
                    _ => continue,
                };
                crawl_registry_key(
                    &RegKey::predef(hive),
                    rel,
                    6,
                    hive_name,
                    &sig,
                    &mut seen_paths,
                    &mut items,
                    app,
                );
            }
        }
    }

    Ok(items)
}

const CRAWL_SKIP: &[&str] = &[
    "microsoft", "windows", "classes", "policies", "clients", "oem",
    "program groups", "systemcert", "registeredapplications",
];

/// Descend from an already-matched registry key and collect descendant keys
/// whose names still reference the app. Depth-bounded, never enters system keys.
#[allow(clippy::too_many_arguments)]
fn crawl_registry_key(
    root: &RegKey,
    rel_path: &str,
    depth_left: u32,
    hive_prefix: &str,
    sig: &AppSignals,
    seen: &mut HashSet<String>,
    out: &mut Vec<LeftoverItem>,
    app: &AppInfo,
) {
    if depth_left == 0 {
        return;
    }
    let key = match root.open_subkey_with_flags(rel_path, KEY_READ) {
        Ok(k) => k,
        Err(_) => return,
    };

    for sub in key.enum_keys().filter_map(|k| k.ok()) {
        let lower = sub.to_lowercase();
        let matched = match_name(sig, &lower);

        if CRAWL_SKIP.iter().any(|s| *s == lower) && matched == MatchKind::None {
            continue;
        }

        let child_rel = format!("{}\\{}", rel_path, sub);
        if matched != MatchKind::None {
            let full_path = format!("{}\\{}", hive_prefix, child_rel);
            if seen.insert(full_path.to_lowercase()) {
                out.push(LeftoverItem {
                    id: crate::models::make_leftover_id(&LeftoverType::Registry, &full_path),
                    path: full_path,
                    item_type: LeftoverType::Registry,
                    confidence: 0,
                    confidence_tier: crate::models::ConfidenceTier::Low,
                    associated_app: app.name.clone(),
                    size: None,
                });
            }
        }

        crawl_registry_key(
            root,
            &child_rel,
            depth_left - 1,
            hive_prefix,
            sig,
            seen,
            out,
            app,
        );
    }
}

