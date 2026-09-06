use crate::engine::confidence::{calculate_confidence, tier_from_score};
use crate::engine::file_scan::scan_files;
use crate::engine::hosts_scan::scan_hosts;
use crate::engine::registry_scan::scan_registry;
use crate::engine::service_scan::scan_services;
use crate::engine::startup_scan::scan_startup;
use crate::engine::task_scan::scan_tasks;
use crate::models::{AppInfo, LeftoverItem, ScanResult};
use std::collections::HashSet;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerifyResult {
    pub remaining: ScanResult,
    pub remaining_count: usize,
    /// Number of delete attempts confirmed gone (deleted or already absent at delete time).
    pub deleted_count: usize,
    /// Items that still exist even though deletion was reported successful.
    pub failed_items: Vec<LeftoverItem>,
}

fn run_verify(app: AppInfo, scan_depth: String, deleted_ids: Vec<String>) -> Result<VerifyResult, String> {
    let mut files = scan_files(&app, &scan_depth).unwrap_or_default();
    let mut registry = scan_registry(&app, &scan_depth).unwrap_or_default();
    let mut services = scan_services(&app).unwrap_or_default();
    let mut startup = scan_startup(&app).unwrap_or_default();
    let mut hosts = scan_hosts(&app).unwrap_or_default();
    let mut tasks = scan_tasks(&app).unwrap_or_default();

    for item in files
        .iter_mut()
        .chain(registry.iter_mut())
        .chain(services.iter_mut())
        .chain(startup.iter_mut())
        .chain(hosts.iter_mut())
        .chain(tasks.iter_mut())
    {
        item.confidence = calculate_confidence(item, &app);
        item.confidence_tier = tier_from_score(item.confidence);
    }

    let attempted: HashSet<String> = deleted_ids.into_iter().collect();

    // Items the delete step reported gone but a fresh scan still finds.
    let still_present = |items: &[LeftoverItem]| -> Vec<LeftoverItem> {
        items
            .iter()
            .filter(|i| attempted.contains(&i.id))
            .cloned()
            .collect()
    };

    let failed_items: Vec<LeftoverItem> = {
        let mut v = Vec::new();
        v.extend(still_present(&files));
        v.extend(still_present(&registry));
        v.extend(still_present(&services));
        v.extend(still_present(&startup));
        v.extend(still_present(&hosts));
        v.extend(still_present(&tasks));
        v
    };

    let failed_set: HashSet<String> = failed_items.iter().map(|i| i.id.clone()).collect();
    let deleted_count = attempted.len().saturating_sub(failed_set.len());

    // remaining = everything the fresh scan finds that was not part of this delete batch
    let remaining = ScanResult {
        files: files.into_iter().filter(|i| !attempted.contains(&i.id)).collect(),
        registry: registry.into_iter().filter(|i| !attempted.contains(&i.id)).collect(),
        services: services.into_iter().filter(|i| !attempted.contains(&i.id)).collect(),
        startup: startup.into_iter().filter(|i| !attempted.contains(&i.id)).collect(),
        hosts: hosts.into_iter().filter(|i| !attempted.contains(&i.id)).collect(),
        tasks: tasks.into_iter().filter(|i| !attempted.contains(&i.id)).collect(),
    };
    let remaining_count = remaining.files.len()
        + remaining.registry.len()
        + remaining.services.len()
        + remaining.startup.len()
        + remaining.hosts.len()
        + remaining.tasks.len();

    Ok(VerifyResult {
        remaining,
        remaining_count,
        deleted_count,
        failed_items,
    })
}

#[tauri::command]
pub async fn verify_scan(app: AppInfo, deleted_ids: Vec<String>, scan_depth: Option<String>) -> Result<VerifyResult, String> {
    let depth = scan_depth.unwrap_or_else(|| "thorough".to_string());
    tokio::task::spawn_blocking(move || run_verify(app, depth, deleted_ids))
        .await
        .map_err(|e| format!("Verify task failed: {}", e))?
}
