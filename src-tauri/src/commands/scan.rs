use crate::engine::confidence::{calculate_confidence, tier_from_score};
use crate::engine::file_scan::scan_files;
use crate::engine::hosts_scan::scan_hosts;
use crate::engine::registry_scan::scan_registry;
use crate::engine::service_scan::scan_services;
use crate::engine::startup_scan::scan_startup;
use crate::engine::task_scan::scan_tasks;
use crate::models::{AppInfo, LeftoverItem, ScanResult};
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub async fn scan_leftovers(
    app: AppHandle,
    app_info: AppInfo,
    excluded_paths: Option<Vec<String>>,
    excluded_hosts: Option<Vec<String>>,
    scan_depth: Option<String>,
) -> Result<ScanResult, String> {
    let _ = app.emit("scan-progress", serde_json::json!({"stage": "scanning", "percent": 10}));

    // User setting: "fast" = shallow crawl; "thorough" (default) = full depth
    // so nested leftovers still surface before the app is removed.
    let depth = match scan_depth.as_deref() {
        Some("fast") => "fast".to_string(),
        _ => "force".to_string(),
    };
    // Engine closures run on blocking threads; each needs its own owned copy.
    let depth_for_registry = depth.clone();
    let excl_paths = excluded_paths.unwrap_or_default();
    let excl_hosts = excluded_hosts.unwrap_or_default();
    let app_info_files = app_info.clone();
    let app_info_registry = app_info.clone();
    let app_info_services = app_info.clone();
    let app_info_startup = app_info.clone();
    let app_info_hosts = app_info.clone();
    let app_info_tasks = app_info.clone();
    let ep_files = excl_paths.clone();
    let ep_registry = excl_paths.clone();
    let ep_services = excl_paths.clone();
    let ep_startup = excl_paths.clone();
    let ep_tasks = excl_paths.clone();
    let eh_hosts = excl_hosts.clone();

    let files_handle = tokio::task::spawn_blocking(move || {
        let mut v = scan_files(&app_info_files, &depth).unwrap_or_default();
        v.retain(|i| !crate::engine::exclude::is_excluded_path(&i.path, &ep_files));
        // Hard guard: Windows-owned locations are never app leftovers,
        // regardless of what name matching thought.
        v.retain(|i| !crate::engine::matching::is_windows_owned_path(&i.path));
        v
    });

    let registry_handle = tokio::task::spawn_blocking(move || {
        let mut v = scan_registry(&app_info_registry, &depth_for_registry).unwrap_or_default();
        v.retain(|i| !crate::engine::exclude::is_excluded_path(&i.path, &ep_registry));
        v
    });

    let services_handle = tokio::task::spawn_blocking(move || {
        let mut v = scan_services(&app_info_services).unwrap_or_default();
        v.retain(|i| !crate::engine::exclude::is_excluded_path(&i.path, &ep_services));
        v
    });

    let startup_handle = tokio::task::spawn_blocking(move || {
        let mut v = scan_startup(&app_info_startup).unwrap_or_default();
        v.retain(|i| !crate::engine::exclude::is_excluded_path(&i.path, &ep_startup));
        v
    });

    let hosts_handle = tokio::task::spawn_blocking(move || {
        let mut v = scan_hosts(&app_info_hosts).unwrap_or_default();
        v.retain(|i| !crate::engine::exclude::is_excluded_host(&i.path, &eh_hosts));
        v
    });

    let tasks_handle = tokio::task::spawn_blocking(move || {
        let mut v = scan_tasks(&app_info_tasks).unwrap_or_default();
        v.retain(|i| !crate::engine::exclude::is_excluded_path(&i.path, &ep_tasks));
        v
    });

    let _ = app.emit("scan-progress", serde_json::json!({"stage": "scanning", "percent": 20}));

    let (files, registry, services, startup, hosts, tasks) = tokio::join!(
        files_handle,
        registry_handle,
        services_handle,
        startup_handle,
        hosts_handle,
        tasks_handle
    );

    let _ = app.emit("scan-progress", serde_json::json!({"stage": "scoring", "percent": 85}));

    // A panicked worker must fail the scan, never silently present a
    // partial result as a clean machine.
    let joined = [
        ("files", files),
        ("registry", registry),
        ("services", services),
        ("startup", startup),
        ("hosts", hosts),
        ("tasks", tasks),
    ];
    let mut failed: Vec<String> = Vec::new();
    let mut lists: Vec<Vec<LeftoverItem>> = Vec::new();
    for (name, res) in joined {
        match res {
            Ok(v) => lists.push(v),
            Err(e) => failed.push(format!("{} ({})", name, e)),
        }
    }
    if !failed.is_empty() {
        return Err(format!(
            "Scan workers failed: {}. Showing nothing instead of a false clean result.",
            failed.join(", ")
        ));
    }
    let [mut files, mut registry, mut services, mut startup, mut hosts, mut tasks]: [Vec<LeftoverItem>; 6] =
        lists.try_into().map_err(|_| "Scan internal error".to_string())?;

    for item in files
        .iter_mut()
        .chain(registry.iter_mut())
        .chain(services.iter_mut())
        .chain(startup.iter_mut())
        .chain(hosts.iter_mut())
        .chain(tasks.iter_mut())
    {
        let scored = calculate_confidence(item, &app_info);
        // Hosts/Task scanners detect under System32 by design and set their
        // own confidence from match strength — keep it when the generic
        // scorer (which penalizes system locations) would erase it.
        item.confidence = item.confidence.max(scored);
        item.confidence_tier = tier_from_score(item.confidence);
    }

    let _ = app.emit("scan-progress", serde_json::json!({"stage": "done", "percent": 100}));

    Ok(ScanResult {
        files,
        registry,
        services,
        startup,
        hosts,
        tasks,
    })
}
