use crate::models::{AttentionItem, DeleteResult, LeftoverItem, LeftoverType};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use winreg::enums::*;
use winreg::RegKey;

#[tauri::command]
pub async fn delete_items(app: AppHandle, items: Vec<LeftoverItem>, create_restore_point: bool, force_kill_allowed: Option<bool>, force_registry_ids: Option<Vec<String>>, allow_without_restore_point: Option<bool>) -> Result<DeleteResult, String> {
    let _ = app.emit("delete-progress", serde_json::json!({"stage": "start", "current": 0, "total": items.len(), "percent": 0}));

    let result = tokio::task::spawn_blocking(move || {
        let mut deleted = 0;
        let mut skipped = 0;
        let mut skipped_items: Vec<crate::models::SkippedItem> = Vec::new();
        let mut already_gone = 0usize;
        let mut already_gone_paths: Vec<String> = Vec::new();
        let mut attention_items: Vec<AttentionItem> = Vec::new();
        let mut deleted_ids: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        // Restore point is tracked separately from item failures: a skipped
        // checkpoint (e.g. not elevated, System Protection off) is not a
        // failed deletion and must never read as one in the UI.
        let mut restore_point_ok = false;
        let mut restore_point_error: Option<String> = None;
        // Registry leaf ids the user explicitly approved in the delete dialog.
        let force_registry_ids: Vec<String> = force_registry_ids.unwrap_or_default();

        // Records a skip with the exact rule that caused it.
        let mut skip = |item: &LeftoverItem, reason: &str| {
            skipped += 1;
            skipped_items.push(crate::models::SkippedItem {
                path: item.path.clone(),
                reason: reason.to_string(),
            });
        };

        if create_restore_point {
            let _ = app.emit("delete-progress", serde_json::json!({"stage": "restore-point", "current": 0, "total": items.len(), "percent": 5}));
            match create_system_restore_point() {
                Ok(_) => restore_point_ok = true,
                Err(e) => restore_point_error = Some(e),
            }
            // ponytail: block-on-restore-fail beats silent proceed; ceiling is one extra checkbox, upgrade is automatic retry.
            if !restore_point_ok && allow_without_restore_point != Some(true) {
                let msg = restore_point_error.clone().unwrap_or_else(|| "Restore point failed".to_string());
                crate::commands::log_event("warn", &format!("delete blocked restore failed: {}", msg));
                return DeleteResult {
                    deleted: 0,
                    skipped: 0,
                    skipped_items: Vec::new(),
                    already_gone: 0,
                    already_gone_paths: Vec::new(),
                    attention_items: Vec::new(),
                    deleted_ids: Vec::new(),
                    errors: vec![format!("Restore point failed — deletion blocked: {}. Re-run with override to proceed without a restore point.", msg)],
                    restore_point_ok: false,
                    restore_point_error,
                };
            }
        }

        // ---- Destructive file plan: collect every file up front so progress
        // is real and one locked file never fails the whole operation. ----
        // (top_idx, file_path)
        let mut file_work: Vec<(usize, PathBuf)> = Vec::new();
        // (top_idx, dir_path, depth) — removed deepest-first after files.
        let mut dir_work: Vec<(usize, PathBuf, usize)> = Vec::new();
        for (idx, item) in items.iter().enumerate() {
            match item.item_type {
                LeftoverType::File | LeftoverType::Folder => {
                    if is_protected_path(&item.path) {
                        continue;
                    }
                    let p = Path::new(&item.path);
                    if !p.exists() {
                        continue;
                    }
                    // Symlinks / metadata failures: treat as single file unit.
                    let md = std::fs::symlink_metadata(p);
                    let is_dir = md.map(|m| m.file_type().is_dir()).unwrap_or(false)
                        && !std::fs::symlink_metadata(p).map(|m| m.file_type().is_symlink()).unwrap_or(false);
                    if !is_dir {
                        file_work.push((idx, p.to_path_buf()));
                    } else {
                        // Walk children independently. Stay on the same
                        // volume so junctions/reparse points cannot pull
                        // deletions onto another drive. Children get a
                        // protected-path recheck at delete time.
                        let walker = walkdir::WalkDir::new(p)
                            .follow_links(false)
                            .same_file_system(true)
                            .min_depth(1);
                        for entry in walker {
                            match entry {
                                Ok(e) => {
                                    let ep = e.path().to_path_buf();
                                    // Skip symlinked dirs as single units (delete link, not target).
                                    let ft = e.file_type();
                                    if ft.is_dir() && !ft.is_symlink() {
                                        let depth = e.depth();
                                        dir_work.push((idx, ep, depth));
                                    } else if ft.is_dir() {
                                        // Symlinked dir: treat as file unit.
                                        file_work.push((idx, ep));
                                    } else {
                                        file_work.push((idx, ep));
                                    }
                                }
                                Err(e) => {
                                    // Unreadable child: record attention, keep going.
                                    let bad = e.path().map(|x| x.to_string_lossy().to_string())
                                        .unwrap_or_else(|| item.path.clone());
                                    attention_items.push(AttentionItem {
                                        path: bad,
                                        reason: format!("Could not list directory entry: {}", e),
                                        action: "Manual removal required".to_string(),
                                        status: "Failed".to_string(),
                                        item_type: "Folder".to_string(),
                                    });
                                }
                            }
                        }
                        // Root itself removed last.
                        dir_work.push((idx, p.to_path_buf(), 0));
                    }
                }
                _ => {}
            }
        }

        // Progress contract: total = walked files + one unit per selected
        // top-level item (finalization / non-file work). Single files count
        // twice by design (file unit + top unit); folders dominate via files.
        // Frontend seeds from events only.
        let total_units = file_work.len() + items.len();
        let mut done_units: usize = 0;
        let emit_progress = |app: &AppHandle, stage: &str, done: usize, total: usize, path: Option<&str>| {
            let percent = if total == 0 { 100 } else { 5 + (done as f64 / total as f64 * 90.0) as u64 };
            let percent = percent.min(99);
            if let Some(p) = path {
                let _ = app.emit("delete-progress", serde_json::json!({
                    "stage": stage, "current": done, "total": total, "percent": percent, "path": p
                }));
            } else {
                let _ = app.emit("delete-progress", serde_json::json!({
                    "stage": stage, "current": done, "total": total, "percent": percent
                }));
            }
        };

        // Per-top ok flag for file items. False once any child needs attention.
        let mut top_ok = vec![true; items.len()];
        // Track tops that already resolved as skip/gone so finalization skips them.
        let mut top_resolved = vec![false; items.len()];

        // Instant outcomes first (protected / already gone) — each counts 1 unit.
        for (idx, item) in items.iter().enumerate() {
            match item.item_type {
                LeftoverType::File | LeftoverType::Folder => {
                    if is_protected_path(&item.path) {
                        skip(item, "Protected Windows location");
                        top_resolved[idx] = true;
                        done_units += 1;
                        emit_progress(&app, "files", done_units, total_units.max(1), Some(&item.path));
                        continue;
                    }
                    if !Path::new(&item.path).exists() {
                        already_gone += 1;
                        already_gone_paths.push(item.path.clone());
                        top_resolved[idx] = true;
                        done_units += 1;
                        emit_progress(&app, "files", done_units, total_units.max(1), Some(&item.path));
                        continue;
                    }
                }
                _ => {}
            }
        }

        // Mark tops with walk-listing attention as not-ok (they already have entries).
        // Walk errors pushed above without top attribution; attribute conservatively:
        // if attention exists for a path under a top root, mark that top not-ok.
        // (Cheap pass; keeps partial folders out of the deleted count.)
        if !attention_items.is_empty() {
            for (idx, item) in items.iter().enumerate() {
                if top_resolved[idx] {
                    continue;
                }
                if matches!(item.item_type, LeftoverType::File | LeftoverType::Folder) {
                    let root = item.path.to_lowercase();
                    let prefix = format!("{}\\", root);
                    for a in attention_items.iter() {
                        let al = a.path.to_lowercase();
                        if al == root || al.starts_with(&prefix) {
                            top_ok[idx] = false;
                            break;
                        }
                    }
                }
            }
        }

        // ---- Phase 1: delete files one by one, never aborting on failure. ----
        let file_total = file_work.len();
        for (fi, (top_idx, fpath)) in file_work.iter().enumerate() {
            let fstr = fpath.to_string_lossy().to_string();
            // Defense-in-depth: walked children get the same system-dir
            // guard as top-level items (junction escape safety).
            if is_protected_path(&fstr) {
                top_ok[*top_idx] = false;
                attention_items.push(AttentionItem {
                    path: fstr.clone(),
                    reason: "Protected Windows location".to_string(),
                    action: "Manual removal required".to_string(),
                    status: "Skipped".to_string(),
                    item_type: "File".to_string(),
                });
                done_units += 1;
                let is_last = fi + 1 == file_total;
                if fi % 10 == 0 || is_last {
                    emit_progress(&app, "files", done_units, total_units.max(1), Some(&fstr));
                }
                continue;
            }
            match delete_single_file(fpath) {
                FileDeleteOutcome::Deleted => {}
                FileDeleteOutcome::Locked(reason) => {
                    // Locked files: try reboot scheduling before giving up.
                    if schedule_delete_on_reboot(&fstr) {
                        top_ok[*top_idx] = false;
                        attention_items.push(AttentionItem {
                            path: fstr.clone(),
                            reason,
                            action: "Restart required".to_string(),
                            status: "Will be deleted after restart".to_string(),
                            item_type: "File".to_string(),
                        });
                    } else {
                        top_ok[*top_idx] = false;
                        attention_items.push(AttentionItem {
                            path: fstr.clone(),
                            reason,
                            action: "Close application".to_string(),
                            status: "Locked".to_string(),
                            item_type: "File".to_string(),
                        });
                    }
                }
                FileDeleteOutcome::Failed(reason) => {
                    top_ok[*top_idx] = false;
                    attention_items.push(AttentionItem {
                        path: fstr.clone(),
                        reason,
                        action: "Manual removal required".to_string(),
                        status: "Failed".to_string(),
                        item_type: "File".to_string(),
                    });
                }
            }
            done_units += 1;
            let is_last = fi + 1 == file_total;
            if fi % 10 == 0 || is_last {
                emit_progress(&app, "files", done_units, total_units.max(1), Some(&fstr));
            }
        }

        // ---- Phase 2: remove dirs deepest-first (only when empty). ----
        dir_work.sort_by_key(|a| std::cmp::Reverse(a.2));
        for (top_idx, dpath, _depth) in dir_work.iter() {
            if top_resolved[*top_idx] {
                continue;
            }
            let dstr = dpath.to_string_lossy().to_string();
            // Guard walked dirs the same way as files (junction safety).
            if is_protected_path(&dstr) {
                top_ok[*top_idx] = false;
                attention_items.push(AttentionItem {
                    path: dstr,
                    reason: "Protected Windows location".to_string(),
                    action: "Manual removal required".to_string(),
                    status: "Skipped".to_string(),
                    item_type: "Folder".to_string(),
                });
                continue;
            }
            // If children failed, dir won't be empty — leave it, attention
            // already recorded for the children.
            match std::fs::remove_dir(dpath) {
                Ok(_) => {}
                Err(e) => {
                    let kind = e.kind();
                    // NotEmpty / AlreadyExists means partial children remain.
                    // Stay silent when child attention exists; otherwise the
                    // summary would read "0 file(s) could not be removed".
                    if kind == std::io::ErrorKind::DirectoryNotEmpty
                        || kind == std::io::ErrorKind::AlreadyExists
                    {
                        top_ok[*top_idx] = false;
                        let is_root = items[*top_idx].path.eq_ignore_ascii_case(&dstr);
                        let has_child_attention = attention_items.iter().any(|a| {
                            let al = a.path.to_lowercase();
                            let rl = dstr.to_lowercase();
                            al != rl && al.starts_with(&format!("{}\\", rl))
                        });
                        if is_root && !has_child_attention {
                            attention_items.push(AttentionItem {
                                path: dstr,
                                reason: format!("Folder not empty: {}", e),
                                action: "Manual removal required".to_string(),
                                status: "Failed".to_string(),
                                item_type: "Folder".to_string(),
                            });
                        }
                        continue;
                    }
                    if is_lock_error(&e) {
                        if schedule_delete_on_reboot(&dstr) {
                            top_ok[*top_idx] = false;
                            attention_items.push(AttentionItem {
                                path: dstr,
                                reason: format!("Folder locked: {}", e),
                                action: "Restart required".to_string(),
                                status: "Will be deleted after restart".to_string(),
                                item_type: "Folder".to_string(),
                            });
                        } else {
                            top_ok[*top_idx] = false;
                            let is_root = items[*top_idx].path.eq_ignore_ascii_case(&dstr);
                            if is_root {
                                attention_items.push(AttentionItem {
                                    path: dstr,
                                    reason: format!("Folder locked: {}", e),
                                    action: "Close application".to_string(),
                                    status: "Locked".to_string(),
                                    item_type: "Folder".to_string(),
                                });
                            }
                        }
                    } else if kind == std::io::ErrorKind::NotFound {
                        // Gone between walk and delete — fine.
                    } else {
                        top_ok[*top_idx] = false;
                        // Only surface root-level dir failures to avoid noise.
                        let is_root = items[*top_idx].path.eq_ignore_ascii_case(&dstr);
                        if is_root {
                            attention_items.push(AttentionItem {
                                path: dstr,
                                reason: format!("Could not remove folder: {}", e),
                                action: "Manual removal required".to_string(),
                                status: "Failed".to_string(),
                                item_type: "Folder".to_string(),
                            });
                        }
                    }
                }
            }
        }

        // ---- Phase 3: finalize file tops + process non-file items. ----
        for (idx, item) in items.iter().enumerate() {
            match item.item_type {
                LeftoverType::File | LeftoverType::Folder => {
                    if top_resolved[idx] {
                        continue;
                    }
                    let stage = "files";
                    let current = done_units + 1;
                    let percent = if total_units == 0 { 100 } else { 5 + (current as f64 / total_units.max(1) as f64 * 90.0) as u64 };
                    let _ = app.emit("delete-progress", serde_json::json!({
                        "stage": stage, "current": current.min(total_units.max(1)),
                        "total": total_units.max(1), "percent": percent.min(99), "path": item.path
                    }));
                    done_units += 1;

                    if top_ok[idx] {
                        // Verify the root is actually gone (or scheduled shouldn't happen here).
                        if !Path::new(&item.path).exists() {
                            deleted += 1;
                            deleted_ids.push(item.id.clone());
                        } else {
                            // Edge: walk found nothing but root still present
                            // (e.g. empty dir whose remove_dir silently failed).
                            // One last direct attempt.
                            let rp = Path::new(&item.path);
                            let last_try = if rp.is_dir() { std::fs::remove_dir(rp) } else { std::fs::remove_file(rp).map(|_| ()) };
                            match last_try {
                                Ok(_) => {
                                    deleted += 1;
                                    deleted_ids.push(item.id.clone());
                                }
                                Err(e) => {
                                    let estr = e.to_string();
                                    if is_lock_error(&e) && schedule_delete_on_reboot(&item.path) {
                                        attention_items.push(AttentionItem {
                                            path: item.path.clone(),
                                            reason: format!("Locked: {}", estr),
                                            action: "Restart required".to_string(),
                                            status: "Will be deleted after restart".to_string(),
                                            item_type: format!("{:?}", item.item_type),
                                        });
                                    } else if is_lock_error(&e) {
                                        attention_items.push(AttentionItem {
                                            path: item.path.clone(),
                                            reason: format!("Locked: {}", estr),
                                            action: "Close application".to_string(),
                                            status: "Locked".to_string(),
                                            item_type: format!("{:?}", item.item_type),
                                        });
                                    } else {
                                        attention_items.push(AttentionItem {
                                            path: item.path.clone(),
                                            reason: format!("Could not delete: {}", estr),
                                            action: "Manual removal required".to_string(),
                                            status: "Failed".to_string(),
                                            item_type: format!("{:?}", item.item_type),
                                        });
                                    }
                                    errors.push(format!("Failed to delete {}: {}", item.path, estr));
                                }
                            }
                        }
                    } else {
                        // Partial: summarize once; details already in attention_items.
                        let remaining: Vec<&AttentionItem> = attention_items.iter()
                            .filter(|a| {
                                let al = a.path.to_lowercase();
                                let rl = item.path.to_lowercase();
                                al == rl || al.starts_with(&format!("{}\\", rl))
                            })
                            .collect();
                        let reboot_count = remaining.iter().filter(|a| a.status == "Will be deleted after restart").count();
                        let locked_count = remaining.iter().filter(|a| a.status == "Locked").count();
                        let failed_count = remaining.len().saturating_sub(reboot_count + locked_count);
                        if reboot_count > 0 && locked_count == 0 && failed_count == 0 {
                            errors.push(format!(
                                "Partial delete {}: {} file(s) scheduled for removal after restart",
                                item.path, reboot_count
                            ));
                        } else if reboot_count > 0 {
                            errors.push(format!(
                                "Partial delete {}: {} locked, {} failed, {} scheduled after restart",
                                item.path, locked_count, failed_count, reboot_count
                            ));
                        } else if locked_count > 0 && failed_count == 0 {
                            errors.push(format!(
                                "Partial delete {}: {} file(s) locked",
                                item.path, locked_count
                            ));
                        } else {
                            errors.push(format!(
                                "Partial delete {}: {} file(s) could not be removed ({} locked, {} failed)",
                                item.path,
                                remaining.len(),
                                locked_count,
                                failed_count
                            ));
                        }
                    }
                }
                LeftoverType::Registry => {
                    if is_protected_registry(&item.path) {
                        skip(item, "Protected Windows registry key");
                        done_units += 1;
                        emit_progress(&app, "registry", done_units, total_units.max(1), Some(&item.path));
                        continue;
                    }
                    // Overridable leaf (Uninstall\{App}): needs explicit
                    // per-delete consent. Without it, skip with a reason
                    // that tells the user approval unlocks it.
                    if is_overridable_registry(&item.path) && !force_registry_ids.contains(&item.id) {
                        skip(item, "Protected registry key — needs your approval");
                        done_units += 1;
                        emit_progress(&app, "registry", done_units, total_units.max(1), Some(&item.path));
                        continue;
                    }

                    // Reversible deletions: snapshot the key tree first. If the
                    // backup cannot be written, refuse to delete. A missing
                    // key (Ok(None)) means already gone, not a failure.
                    match crate::engine::registry_backup::backup_registry_path(&item.path) {
                        Ok(Some(_)) => match delete_registry_entry(&item.path) {
                            Ok(true) => { deleted += 1; deleted_ids.push(item.id.clone()); }
                            Ok(false) => {
                                already_gone += 1;
                                already_gone_paths.push(item.path.clone());
                            }
                            Err(e) => {
                                errors.push(format!("Failed to delete registry {}: {}", item.path, e));
                                attention_items.push(AttentionItem {
                                    path: item.path.clone(),
                                    reason: e,
                                    action: "Manual removal required".to_string(),
                                    status: "Failed".to_string(),
                                    item_type: "Registry".to_string(),
                                });
                            }
                        },
                        Ok(None) => {
                            already_gone += 1;
                            already_gone_paths.push(item.path.clone());
                        }
                        Err(e) => {
                            let msg = format!(
                                "Registry backup failed — delete skipped for {}: {}",
                                item.path, e
                            );
                            errors.push(msg.clone());
                            attention_items.push(AttentionItem {
                                path: item.path.clone(),
                                reason: msg,
                                action: "Manual removal required".to_string(),
                                status: "Failed".to_string(),
                                item_type: "Registry".to_string(),
                            });
                        }
                    }
                    done_units += 1;
                    emit_progress(&app, "registry", done_units, total_units.max(1), Some(&item.path));
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
                            Ok(Some(_)) => match delete_startup_registry(path_str) {
                                Ok(true) => { deleted += 1; deleted_ids.push(item.id.clone()); }
                                Ok(false) => {
                                    already_gone += 1;
                                    already_gone_paths.push(path_str.clone());
                                }
                                Err(e) => {
                                    errors.push(format!("Failed to delete startup entry {}: {}", path_str, e));
                                    attention_items.push(AttentionItem {
                                        path: path_str.clone(),
                                        reason: e,
                                        action: "Manual removal required".to_string(),
                                        status: "Failed".to_string(),
                                        item_type: "Startup".to_string(),
                                    });
                                }
                            },
                            Ok(None) => {
                                already_gone += 1;
                                already_gone_paths.push(path_str.clone());
                            }
                            Err(e) => {
                                let msg = format!(
                                    "Registry backup failed — startup delete skipped for {}: {}",
                                    path_str, e
                                );
                                errors.push(msg.clone());
                                attention_items.push(AttentionItem {
                                    path: path_str.clone(),
                                    reason: msg,
                                    action: "Manual removal required".to_string(),
                                    status: "Failed".to_string(),
                                    item_type: "Startup".to_string(),
                                });
                            }
                        }
                        done_units += 1;
                        emit_progress(&app, "startup", done_units, total_units.max(1), Some(path_str));
                    } else {
                        // File-based startup entry: same safety + direct delete.
                        if is_protected_path(path_str) {
                            skip(item, "Protected Windows location");
                            done_units += 1;
                            emit_progress(&app, "startup", done_units, total_units.max(1), Some(path_str));
                            continue;
                        }
                        let path = Path::new(path_str);
                        // Symlink metadata: a startup dir or symlink-to-dir
                        // must never read as already-gone while on disk.
                        let is_present = std::fs::symlink_metadata(path).is_ok();
                        let is_plain_file = path.is_file()
                            && !std::fs::symlink_metadata(path)
                                .map(|m| m.file_type().is_symlink())
                                .unwrap_or(false);
                        if is_present && !is_plain_file {
                            // Directory startup entry: remove when empty,
                            // otherwise report — never force recursive delete
                            // of an unrelated tree here.
                            match std::fs::remove_dir(path) {
                                Ok(_) => {
                                    deleted += 1;
                                    deleted_ids.push(item.id.clone());
                                }
                                Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                                    attention_items.push(AttentionItem {
                                        path: path_str.clone(),
                                        reason: "Startup folder not empty".to_string(),
                                        action: "Manual removal required".to_string(),
                                        status: "Failed".to_string(),
                                        item_type: "Startup".to_string(),
                                    });
                                    errors.push(format!("Startup folder not empty {}: remove contents first", path_str));
                                }
                                Err(e) if is_lock_error(&e) => {
                                    if schedule_delete_on_reboot(path_str) {
                                        attention_items.push(AttentionItem {
                                            path: path_str.clone(),
                                            reason: e.to_string(),
                                            action: "Restart required".to_string(),
                                            status: "Will be deleted after restart".to_string(),
                                            item_type: "Startup".to_string(),
                                        });
                                        errors.push(format!("Startup folder scheduled after restart: {}", path_str));
                                    } else {
                                        attention_items.push(AttentionItem {
                                            path: path_str.clone(),
                                            reason: e.to_string(),
                                            action: "Close application".to_string(),
                                            status: "Locked".to_string(),
                                            item_type: "Startup".to_string(),
                                        });
                                        errors.push(format!("Startup folder locked {}: {}", path_str, e));
                                    }
                                }
                                Err(e) => {
                                    attention_items.push(AttentionItem {
                                        path: path_str.clone(),
                                        reason: e.to_string(),
                                        action: "Manual removal required".to_string(),
                                        status: "Failed".to_string(),
                                        item_type: "Startup".to_string(),
                                    });
                                    errors.push(format!("Failed to delete startup folder {}: {}", path_str, e));
                                }
                            }
                        } else if is_plain_file {
                            match delete_single_file(path) {
                                FileDeleteOutcome::Deleted => {
                                    deleted += 1;
                                    deleted_ids.push(item.id.clone());
                                }
                                FileDeleteOutcome::Locked(reason) => {
                                    if schedule_delete_on_reboot(path_str) {
                                        attention_items.push(AttentionItem {
                                            path: path_str.clone(),
                                            reason,
                                            action: "Restart required".to_string(),
                                            status: "Will be deleted after restart".to_string(),
                                            item_type: "Startup".to_string(),
                                        });
                                        errors.push(format!("Startup file scheduled after restart: {}", path_str));
                                    } else {
                                        attention_items.push(AttentionItem {
                                            path: path_str.clone(),
                                            reason: reason.clone(),
                                            action: "Close application".to_string(),
                                            status: "Locked".to_string(),
                                            item_type: "Startup".to_string(),
                                        });
                                        errors.push(format!("Startup file locked {}: {}", path_str, reason));
                                    }
                                }
                                FileDeleteOutcome::Failed(reason) => {
                                    attention_items.push(AttentionItem {
                                        path: path_str.clone(),
                                        reason: reason.clone(),
                                        action: "Manual removal required".to_string(),
                                        status: "Failed".to_string(),
                                        item_type: "Startup".to_string(),
                                    });
                                    errors.push(format!("Failed to delete startup file {}: {}", path_str, reason));
                                }
                            }
                        } else {
                            already_gone += 1;
                            already_gone_paths.push(item.path.clone());
                        }
                        done_units += 1;
                        emit_progress(&app, "startup", done_units, total_units.max(1), Some(path_str));
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
                        done_units += 1;
                        emit_progress(&app, "services", done_units, total_units.max(1), Some(&item.path));
                        continue;
                    }

                    match delete_windows_service(service_name, force_kill_allowed.unwrap_or(false)) {
                        Ok(_) => { deleted += 1; deleted_ids.push(item.id.clone()); }
                        Err(e) => {
                            errors.push(format!("Failed to delete service {}: {}", service_name, e));
                            attention_items.push(AttentionItem {
                                path: item.path.clone(),
                                reason: e,
                                action: "Run as administrator".to_string(),
                                status: "Failed".to_string(),
                                item_type: "Service".to_string(),
                            });
                        }
                    }
                    done_units += 1;
                    emit_progress(&app, "services", done_units, total_units.max(1), Some(&item.path));
                }
                LeftoverType::Hosts | LeftoverType::Task => {
                    // Read-only detection only — never auto-delete System32/hosts or Task files
                    let what = if matches!(item.item_type, LeftoverType::Hosts) {
                        "Hosts entry — ClearOut doesn't edit the hosts file"
                    } else {
                        "Scheduled task — ClearOut doesn't remove scheduled tasks"
                    };
                    skip(item, what);
                    done_units += 1;
                    let stage = if matches!(item.item_type, LeftoverType::Hosts) { "hosts" } else { "tasks" };
                    emit_progress(&app, stage, done_units, total_units.max(1), Some(&item.path));
                }
            }
        }

        // Clamp progress to total (rounding from throttled emits).
        let total_out = total_units.max(1);
        let _ = app.emit("delete-progress", serde_json::json!({"stage": "done", "current": total_out, "total": total_out, "percent": 100}));

        DeleteResult {
            deleted,
            skipped,
            skipped_items,
            already_gone,
            already_gone_paths,
            attention_items,
            deleted_ids,
            errors,
            restore_point_ok,
            restore_point_error,
        }
    }).await.map_err(|e| format!("Delete task failed: {}", e))?;

    Ok(result)
}

enum FileDeleteOutcome {
    Deleted,
    Locked(String),
    Failed(String),
}

fn delete_single_file(path: &Path) -> FileDeleteOutcome {
    clear_readonly(path);
    match std::fs::remove_file(path) {
        Ok(_) => FileDeleteOutcome::Deleted,
        Err(e) => {
            if is_lock_error(&e) {
                FileDeleteOutcome::Locked(e.to_string())
            } else {
                // Symlink-to-dir or other special: try remove_dir as fallback
                // (deletes the link itself, never the target).
                if let Ok(ft) = std::fs::symlink_metadata(path).map(|m| m.file_type()) {
                    if ft.is_symlink() && std::fs::remove_dir(path).is_ok() {
                        return FileDeleteOutcome::Deleted;
                    }
                }
                FileDeleteOutcome::Failed(e.to_string())
            }
        }
    }
}

#[allow(clippy::permissions_set_readonly_false)]
fn clear_readonly(path: &Path) {
    if let Ok(md) = std::fs::symlink_metadata(path) {
        let mut perm = md.permissions();
        if perm.readonly() {
            perm.set_readonly(false);
            let _ = std::fs::set_permissions(path, perm);
        }
    }
}

fn is_lock_error(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        return true;
    }
    if let Some(code) = e.raw_os_error() {
        // Windows: 5 Access denied, 32 Sharing violation, 33 Lock violation.
        return code == 5 || code == 32 || code == 33;
    }
    false
}

/// Schedule a locked path for deletion on next reboot via MoveFileEx.
/// Returns true when Windows accepted the schedule. Never forces deletion.
/// Refuses protected system paths even if a caller forgets the check.
fn schedule_delete_on_reboot(path: &str) -> bool {
    if is_protected_path(path) {
        return false;
    }
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
        // HSTRING buffers are not null-terminated; PCWSTR::from_raw on one
        // overreads. Build a terminated wide string instead.
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MoveFileExW(PCWSTR(wide.as_ptr()), PCWSTR::null(), MOVEFILE_DELAY_UNTIL_REBOOT).is_ok()
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// Lexically normalize a Windows path for guard checks: slashes unified,
/// \\?\ prefixes stripped, `.`/`..` resolved, case lowered, trailing
/// backslash dropped (except drive roots). No filesystem access, so it
/// works on missing paths too.
fn normalize_fs_path(path: &str) -> String {
    let mut s = path.replace('/', "\\").to_lowercase();
    for prefix in ["\\\\?\\unc\\", "\\\\?\\", "\\??\\"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = if prefix == "\\\\?\\unc\\" {
                format!("\\\\{}", rest)
            } else {
                rest.to_string()
            };
            break;
        }
    }
    // Split off drive (`c:`) or UNC (`\\server\share`) prefix.
    let bytes = s.as_bytes();
    let mut prefix_end = 0;
    if s.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        prefix_end = 2;
    } else if s.starts_with("\\\\") {
        let mut slashes = 0;
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'\\' {
                slashes += 1;
                if slashes == 4 {
                    prefix_end = i;
                    break;
                }
            }
        }
        if slashes < 4 {
            prefix_end = s.len();
        }
    }
    let (prefix, rest) = s.split_at(prefix_end);
    let mut parts: Vec<&str> = Vec::new();
    for comp in rest.split('\\') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    let mut out = String::from(prefix);
    out.push('\\');
    out.push_str(&parts.join("\\"));
    // Keep `c:\` as-is; drop trailing slash elsewhere.
    while out.len() > 3 && out.ends_with('\\') {
        out.pop();
    }
    out
}

fn is_protected_path(path: &str) -> bool {
    // Block true system dirs on ANY drive — matches confidence.rs + blueprint 5.2.
    // Do NOT block Program Files / ProgramData / Users contents — leftovers live
    // there and must be deletable. But their ROOTS are never valid delete targets.
    let norm = normalize_fs_path(path);
    let bytes = norm.as_bytes();
    // Drive root itself (`c:\`) is never a delete target.
    if norm.len() == 3 && bytes[1] == b':' && norm.ends_with(":\\") {
        return true;
    }
    let drive = if norm.len() >= 2 && bytes[1] == b':' {
        norm[0..2].to_string()
    } else {
        // Non-drive path: fall back to the C: list.
        "c:".to_string()
    };
    let under = |dir: &str| norm == format!("{}{}", drive, dir) || norm.starts_with(&format!("{}{}\\", drive, dir));
    // System dirs: block the dir and everything under it.
    for d in [
        "\\windows",
        "\\windows\\system32",
        "\\windows\\syswow64",
        "\\windows\\sysnative",
        "\\windows\\winsxs",
        "\\windows\\fonts",
        "\\windows\\boot",
        "\\windows\\installer",
        "\\windows\\prefetch",
        "\\windows\\temp",
        "\\windows\\systemresources",
        "\\windows\\servicing",
    ] {
        if under(d) {
            return true;
        }
    }
    // Container roots: block the root itself only, contents stay deletable.
    for r in [
        "\\program files",
        "\\program files (x86)",
        "\\programdata",
        "\\users",
    ] {
        if norm == format!("{}{}", drive, r) {
            return true;
        }
    }
    false
}

/// Hard-protected registry parents: the keys themselves can never be
/// deleted, not even with override. Wiping `...\Uninstall` bricks every
/// other uninstaller; wiping `...\CurrentVersion` breaks login.
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
        "hkey_local_machine\\software\\microsoft\\windows\\currentversion\\run",
        "hkey_current_user\\software\\microsoft\\windows\\currentversion\\run",
        "hkey_local_machine\\software\\microsoft\\windows\\currentversion\\runonce",
        "hkey_current_user\\software\\microsoft\\windows\\currentversion\\runonce",
    ];
    // Exact match only: the parent itself. Leaf app entries nested deeper
    // (Uninstall\{App}) are overridable, never hard-blocked.
    critical.iter().any(|c| lower == *c)
}

/// True when the key nests strictly under a hard-protected parent — a leaf
/// app entry like `...\Uninstall\{AppGuid}`. Deleting it removes one scoped
/// subkey, so explicit user consent plus a mandatory backup makes it safe.
fn is_overridable_registry(path: &str) -> bool {
    let lower = path.to_lowercase();
    if is_protected_registry(path) {
        return false;
    }
    let parents = [
        "hkey_local_machine\\software\\microsoft\\windows\\currentversion\\uninstall",
        "hkey_current_user\\software\\microsoft\\windows\\currentversion\\uninstall",
        "hkey_local_machine\\software\\microsoft\\windows\\currentversion\\run",
        "hkey_current_user\\software\\microsoft\\windows\\currentversion\\run",
        "hkey_local_machine\\software\\microsoft\\windows\\currentversion\\runonce",
        "hkey_current_user\\software\\microsoft\\windows\\currentversion\\runonce",
    ];
    parents
        .iter()
        .any(|p| lower.starts_with(&format!("{}\\", p)))
}

/// Ok(true) deleted, Ok(false) already gone, Err failed. A missing key is
/// gone, not a failure — parent-first selection deletes children implicitly.
fn delete_registry_entry(path: &str) -> Result<bool, String> {
    let parts: Vec<&str> = path.splitn(2, '\\').collect();
    if parts.len() < 2 {
        return Err("Invalid registry path".to_string());
    }

    let (hive_name, subpath) = (parts[0], parts[1]);
    let hive = if hive_name.eq_ignore_ascii_case("HKEY_CURRENT_USER") {
        HKEY_CURRENT_USER
    } else if hive_name.eq_ignore_ascii_case("HKEY_LOCAL_MACHINE") {
        HKEY_LOCAL_MACHINE
    } else {
        return Err(format!("Unsupported hive: {}", hive_name));
    };

    let last_backslash = subpath.rfind('\\').ok_or_else(|| "Invalid subkey path".to_string())?;
    let parent_path = &subpath[..last_backslash];
    let key_to_delete = &subpath[last_backslash + 1..];

    let root = RegKey::predef(hive);
    let parent_key = match root.open_subkey_with_flags(parent_path, KEY_WRITE) {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("Failed to open parent key {}: {}", parent_path, e)),
    };

    match parent_key.delete_subkey_all(key_to_delete) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("Failed to delete subkey {}: {}", key_to_delete, e)),
    }
}

fn delete_startup_registry(entry: &str) -> Result<bool, String> {
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

    let hive = if key_parts[0].eq_ignore_ascii_case("HKEY_CURRENT_USER") {
        HKEY_CURRENT_USER
    } else if key_parts[0].eq_ignore_ascii_case("HKEY_LOCAL_MACHINE") {
        HKEY_LOCAL_MACHINE
    } else {
        return Err("Unsupported hive".to_string());
    };

    let root = RegKey::predef(hive);
    let key = match root.open_subkey_with_flags(key_parts[1], KEY_WRITE) {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("Failed to open key {}: {}", key_parts[1], e)),
    };

    match key.delete_value(value_name) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("Failed to delete startup value {}: {}", value_name, e)),
    }
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
/// a system dir. Fail CLOSED: unreadable ImagePath or unparsable path
/// counts as protected — deleting a critical service bricks boot.
fn is_protected_service(service_name: &str) -> bool {
    match read_service_image_path(service_name) {
        Some(image) => {
            let normalized = crate::engine::service_scan::normalized_image_path(&image);
            if normalized.is_empty() {
                return true;
            }
            crate::engine::service_scan::is_system_service_path(&normalized)
        }
        None => true,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_blocks_system_dirs_any_drive() {
        assert!(is_protected_path(r"C:\Windows\System32\foo.dll"));
        assert!(is_protected_path(r"D:\Windows\System32\foo.dll"));
        assert!(is_protected_path("c:/windows/system32/foo.dll"));
        assert!(is_protected_path(r"\\?\C:\Windows\Temp\x"));
        assert!(is_protected_path(r"C:\Windows\..\Windows\System32\x"));
        assert!(!is_protected_path(r"C:\Program Files\App\app.exe"));
        assert!(!is_protected_path(r"C:\Users\Bob\AppData\Roaming\App"));
    }

    #[test]
    fn guard_blocks_roots_but_not_contents() {
        assert!(is_protected_path(r"C:\"));
        assert!(is_protected_path(r"C:\Program Files"));
        assert!(is_protected_path(r"C:\Program Files (x86)"));
        assert!(is_protected_path(r"C:\ProgramData"));
        assert!(is_protected_path(r"C:\Users"));
        assert!(!is_protected_path(r"C:\Program Files\App"));
        assert!(!is_protected_path(r"C:\Users\Bob"));
    }

    #[test]
    fn registry_parents_hard_blocked_leaves_overridable() {
        assert!(is_protected_registry(r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"));
        assert!(is_protected_registry(r"HKEY_LOCAL_MACHINE\SOFTWARE"));
        assert!(is_protected_registry(r"HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\Run"));
        assert!(!is_protected_registry(r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{12345678-1234-1234-1234-123456789ABC}"));
        assert!(is_overridable_registry(r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{12345678-1234-1234-1234-123456789ABC}"));
        assert!(is_overridable_registry(r"HKEY_CURRENT_USER\SOFTWARE\Microsoft\Windows\CurrentVersion\Run\AppName"));
        assert!(!is_overridable_registry(r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"));
        assert!(!is_overridable_registry(r"HKEY_LOCAL_MACHINE\SOFTWARE\Vendor\App"));
    }

    #[test]
    fn protected_service_fail_closed() {
        // Unknown service reads as protected — never touch what cannot be read.
        assert!(is_protected_service("DefinitelyNotAService12345XYZ"));
    }

    #[test]
    fn normalize_resolves_dots() {
        assert_eq!(normalize_fs_path(r"C:\A\..\Windows\System32\x"), "c:\\windows\\system32\\x");
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegistryImpact {
    pub keys: usize,
    pub values: usize,
}

/// ponytail: count beats full tree send; ceiling is live count (key may change before delete), upgrade is snapshot diff.
#[tauri::command]
pub fn registry_impact(path: String) -> Result<RegistryImpact, String> {
    if is_protected_registry(&path) {
        return Err("Protected registry key".to_string());
    }
    match crate::engine::registry_backup::capture_key_tree(&path) {
        Ok(keys) => {
            let values = keys.iter().map(|k| k.values.len()).sum();
            Ok(RegistryImpact { keys: keys.len(), values })
        }
        Err(e) => {
            // Missing key = zero impact, not an error for preview UX.
            let lower = e.to_lowercase();
            if lower.contains("not found") || lower.contains("failed to open") {
                Ok(RegistryImpact { keys: 0, values: 0 })
            } else {
                Err(e)
            }
        }
    }
}
