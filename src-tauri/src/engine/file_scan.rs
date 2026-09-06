use crate::engine::matching::{match_name, AppSignals, MatchKind};
use crate::models::{AppInfo, LeftoverItem, LeftoverType};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn new_item(
    item_type: &LeftoverType,
    path: String,
    app: &AppInfo,
    size: Option<u64>,
) -> LeftoverItem {
    LeftoverItem {
        id: crate::models::make_leftover_id(item_type, &path),
        path,
        item_type: item_type.clone(),
        confidence: 0,
        confidence_tier: crate::models::ConfidenceTier::Low,
        associated_app: app.name.clone(),
        size,
    }
}

fn entry_matches_kind(
    sig: &AppSignals,
    file_name_lower: &str,
) -> MatchKind {
    match_name(sig, file_name_lower)
}

/// "force" mode: descend a few levels under an already-matched folder and
/// collect nested matches, without descending into system-named dirs.
fn deeper_matches(
    dir: &Path,
    depth_left: u32,
    sig: &AppSignals,
    skip: &[&str],
    out: &mut Vec<(PathBuf, bool, Option<u64>)>,
) {
    if depth_left == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        let p = entry.path();
        let fname = entry.file_name().to_string_lossy().to_lowercase();
        if fname.starts_with('.') {
            continue;
        }
        let matched = entry_matches_kind(sig, &fname) != MatchKind::None;
        let is_dir = p.is_dir();
        let size = if is_dir { None } else { p.metadata().ok().map(|m| m.len()) };
        if is_dir {
            let pruned = !matched && skip.iter().any(|s| &fname == s);
            if !pruned {
                deeper_matches(&p, depth_left - 1, sig, skip, out);
            }
        }
        if matched {
            out.push((p, is_dir, size));
        }
    }
}

pub fn scan_files(app: &AppInfo, scan_depth: &str) -> Result<Vec<LeftoverItem>, String> {
    let mut items: Vec<LeftoverItem> = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();

    let sig = AppSignals::from_app(app);
    let publisher_lower = sig.publisher_lower.clone();

    // 1. Check InstallLocation directly
    if let Some(loc) = &app.install_location {
        let clean_loc = loc.trim().trim_matches('"');
        if !clean_loc.is_empty() {
            let path_obj = Path::new(clean_loc);
            if path_obj.exists() {
                let is_dir = path_obj.is_dir();
                let item_type = if is_dir { LeftoverType::Folder } else { LeftoverType::File };
                let size = if is_dir { None } else { path_obj.metadata().ok().map(|m| m.len()) };
                let loc_str = clean_loc.to_string();

                if seen_paths.insert(loc_str.to_lowercase()) {
                    items.push(new_item(&item_type, loc_str, app, size));
                }
            }
        }
    }

    // 2. Base software search locations
    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        search_dirs.push(PathBuf::from(appdata));
    }
    if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        search_dirs.push(PathBuf::from(&localappdata).join("Programs"));
        search_dirs.push(PathBuf::from(localappdata));
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        search_dirs.push(PathBuf::from(&userprofile).join("AppData").join("LocalLow"));
        search_dirs.push(PathBuf::from(&userprofile).join("Documents"));
        search_dirs.push(PathBuf::from(&userprofile).join("Saved Games"));
    }
    if let Ok(programdata) = std::env::var("PROGRAMDATA") {
        search_dirs.push(PathBuf::from(programdata));
    }
    search_dirs.push(PathBuf::from("C:\\Program Files"));
    search_dirs.push(PathBuf::from("C:\\Program Files (x86)"));

    let system_skip = [
        "microsoft", "windows", "temp", "system32", "syswow64", "packages", "common files",
    ];

    // Parallel scan of base directories
    let all_items: Mutex<Vec<LeftoverItem>> = Mutex::new(Vec::new());
    let all_seen: Mutex<HashSet<String>> = Mutex::new(HashSet::new());

    search_dirs.par_iter().for_each(|base_dir| {
        if !base_dir.exists() || !base_dir.is_dir() {
            return;
        }

        let entries = match std::fs::read_dir(base_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path_obj = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();
            let file_name_lower = file_name.to_lowercase();

            if file_name.starts_with('.') {
                continue;
            }

            let matched = entry_matches_kind(&sig, &file_name_lower);

            if matched != MatchKind::None {
                let path_str = path_obj.to_string_lossy().to_string();
                let mut seen = all_seen.lock().unwrap_or_else(|e| e.into_inner());
                if seen.insert(path_str.to_lowercase()) {
                    drop(seen);
                    let is_dir = path_obj.is_dir();
                    let item_type = if is_dir { LeftoverType::Folder } else { LeftoverType::File };
                    let size = if is_dir { None } else { path_obj.metadata().ok().map(|m| m.len()) };

                    all_items.lock().unwrap_or_else(|e| e.into_inner()).push(new_item(&item_type, path_str, app, size));
                }

                // "force" mode: descend a few levels under the matched folder and
                // collect nested matches too (deeper leftovers).
                if scan_depth == "force" && path_obj.is_dir() {
                    let mut found: Vec<(PathBuf, bool, Option<u64>)> = Vec::new();
                    deeper_matches(
                        &path_obj,
                        2,
                        &sig,
                        &system_skip,
                        &mut found,
                    );
                    for (fp, is_dir, fsize) in found {
                        let fp_str = fp.to_string_lossy().to_string();
                        let mut seen = all_seen.lock().unwrap_or_else(|e| e.into_inner());
                        if seen.insert(fp_str.to_lowercase()) {
                            drop(seen);
                            let itype = if is_dir { LeftoverType::Folder } else { LeftoverType::File };
                            all_items.lock().unwrap_or_else(|e| e.into_inner()).push(new_item(&itype, fp_str, app, fsize));
                        }
                    }
                }
            } else if path_obj.is_dir() && (scan_depth == "thorough" || scan_depth == "force") {
                // Check if this directory is a vendor/publisher directory (e.g. Google, Riot Games, Steam)
                let is_publisher = (!publisher_lower.is_empty() && file_name_lower.contains(&publisher_lower))
                    || sig.distinctive.contains(&file_name_lower);

                if is_publisher && !system_skip.iter().any(|s| &file_name_lower == s) {
                    if let Ok(child_entries) = std::fs::read_dir(&path_obj) {
                        for child in child_entries.filter_map(|e| e.ok()) {
                            let child_path = child.path();
                            let child_name = child.file_name().to_string_lossy().to_string();
                            let child_name_lower = child_name.to_lowercase();

                            if entry_matches_kind(&sig, &child_name_lower) != MatchKind::None {
                                let child_path_str = child_path.to_string_lossy().to_string();
                                let mut seen = all_seen.lock().unwrap_or_else(|e| e.into_inner());
                                if seen.insert(child_path_str.to_lowercase()) {
                                    drop(seen);
                                    let is_dir = child_path.is_dir();
                                    let item_type = if is_dir { LeftoverType::Folder } else { LeftoverType::File };
                                    let size = if is_dir { None } else { child_path.metadata().ok().map(|m| m.len()) };

                                    all_items.lock().unwrap_or_else(|e| e.into_inner()).push(new_item(&item_type, child_path_str, app, size));
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    items.extend(all_items.into_inner().unwrap());
    Ok(items)
}
