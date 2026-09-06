use crate::engine::matching::{match_name, match_text, AppSignals, MatchKind};
use crate::models::{AppInfo, ConfidenceTier, LeftoverItem, LeftoverType};
use std::path::Path;

/// Read-only Scheduled Tasks scanner.
/// Enumerates C:\Windows\System32\Tasks and C:\Windows\SysWOW64\Tasks (if present)
/// by filesystem listing only — no COM, no writes. Matches tasks via the shared
/// token matcher, so Windows-owned names like "OneCore" can never collide with
/// an app keyword (see engine::matching).
pub fn scan_tasks(app: &AppInfo) -> Result<Vec<LeftoverItem>, String> {
    let sig = AppSignals::from_app(app);

    let mut items = Vec::new();

    let task_dirs = [
        Path::new(r"C:\Windows\System32\Tasks"),
        Path::new(r"C:\Windows\SysWOW64\Tasks"),
    ];

    for base in task_dirs {
        if !base.exists() || !base.is_dir() {
            continue;
        }
        // Recursively walk one level deep — tasks can be in subfolders
        let entries = match std::fs::read_dir(base) {
            Ok(e) => e,
            Err(_) => continue, // access denied -> graceful empty
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let name_lower = name.to_lowercase();

            let mut matched = MatchKind::None;
            let mut confidence = 0u32;

            match match_name(&sig, &name_lower) {
                MatchKind::Strong => {
                    matched = MatchKind::Strong;
                    confidence = 80;
                }
                MatchKind::Weak => {
                    matched = MatchKind::Weak;
                    confidence = 65;
                }
                MatchKind::None => {}
            }

            if path.is_dir() {
                // Recurse into subfolder (one level)
                let children = match std::fs::read_dir(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                for child in children.filter_map(|e| e.ok()) {
                    let child_path = child.path();
                    let child_name = child.file_name().to_string_lossy().to_string();
                    let child_lower = child_name.to_lowercase();

                    let mut child_conf: Option<u32> = match match_name(&sig, &child_lower) {
                        MatchKind::Strong => Some(80),
                        MatchKind::Weak => Some(65),
                        MatchKind::None => None,
                    };

                    // Content peek (first 4KB XML) — only a full-name or compact
                    // reference counts. Never a lone keyword: stock Windows task
                    // XMLs are full of generic words like "temp" and "update".
                    if child_conf.is_none() && child_path.is_file() {
                        if let Ok(text) = std::fs::read_to_string(&child_path) {
                            if matches!(match_text(&sig, &text.to_lowercase()), MatchKind::Strong) {
                                child_conf = Some(55);
                            }
                        }
                    }

                    if let Some(conf) = child_conf {
                        let tier = tier_from(conf);
                        let full_path = child_path.to_string_lossy().to_string();
                        items.push(LeftoverItem {
                            id: crate::models::make_leftover_id(&LeftoverType::Task, &full_path),
                            path: full_path,
                            item_type: LeftoverType::Task,
                            confidence: conf,
                            confidence_tier: tier,
                            associated_app: app.name.clone(),
                            size: None,
                        });
                    }
                }
                continue;
            }

            if matched != MatchKind::None {
                let tier = tier_from(confidence);
                let full_path = path.to_string_lossy().to_string();
                items.push(LeftoverItem {
                    id: crate::models::make_leftover_id(&LeftoverType::Task, &full_path),
                    path: full_path,
                    item_type: LeftoverType::Task,
                    confidence,
                    confidence_tier: tier,
                    associated_app: app.name.clone(),
                    size: path.metadata().ok().map(|m| m.len()),
                });
                continue;
            }

            // Name didn't match — peek content for hashed task file names.
            // Full-name reference only (see child content peek above).
            if path.is_file() {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if matches!(match_text(&sig, &text.to_lowercase()), MatchKind::Strong) {
                        let full_path = path.to_string_lossy().to_string();
                        items.push(LeftoverItem {
                            id: crate::models::make_leftover_id(&LeftoverType::Task, &full_path),
                            path: full_path,
                            item_type: LeftoverType::Task,
                            confidence: 55,
                            confidence_tier: ConfidenceTier::Medium,
                            associated_app: app.name.clone(),
                            size: path.metadata().ok().map(|m| m.len()),
                        });
                    }
                }
            }
        }
    }

    // De-duplicate by path
    let mut seen = std::collections::HashSet::new();
    items.retain(|i| seen.insert(i.path.to_lowercase()));
    Ok(items)
}

fn tier_from(confidence: u32) -> ConfidenceTier {
    if confidence >= 70 {
        ConfidenceTier::High
    } else if confidence >= 45 {
        ConfidenceTier::Medium
    } else {
        ConfidenceTier::Low
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AppInfo;

    fn app(name: &str) -> AppInfo {
        AppInfo {
            id: "coretemp".into(),
            name: name.into(),
            version: None,
            publisher: Some("ALCPU".into()),
            install_location: None,
            estimated_size: None,
            uninstall_string: None,
            install_date: None,
            icon: None,
        }
    }

    #[test]
    fn regression_onecore_not_flagged_for_core_temp() {
        let sig = AppSignals::from_app(&app("Core Temp"));
        // The exact false positive from the field report:
        assert_eq!(match_name(&sig, "microsoft"), MatchKind::None);
        assert_eq!(match_name(&sig, "onecore"), MatchKind::None);
        assert_eq!(
            match_text(&sig, r#"<task><triggers><boottrigger/></triggers></task>"#),
            MatchKind::None
        );
    }

    #[test]
    fn genuine_task_still_detected() {
        let sig = AppSignals::from_app(&app("Core Temp"));
        assert_eq!(match_name(&sig, "coretemp"), MatchKind::Strong);
        assert_eq!(match_name(&sig, "core temp"), MatchKind::Strong);
        assert_eq!(match_name(&sig, "coretempstartup"), MatchKind::Strong);
    }
}
