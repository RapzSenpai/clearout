use crate::models::{DeleteResult, ScanResult};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportReport {
    pub app_name: String,
    pub app_version: Option<String>,
    pub timestamp: String,
    pub scan_summary: ScanSummary,
    pub delete_summary: Option<DeleteSummary>,
    pub items: Vec<ExportItem>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanSummary {
    pub total_files: usize,
    pub total_registry: usize,
    pub total_services: usize,
    pub total_startup: usize,
    pub total_hosts: usize,
    pub total_tasks: usize,
    pub total_items: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeleteSummary {
    pub deleted: usize,
    pub skipped: usize,
    /// Every rule-skipped item with the rule that caused the skip.
    pub skipped_items: Vec<crate::models::SkippedItem>,
    /// Items already gone at delete time (nothing left to do).
    pub already_gone: usize,
    pub already_gone_paths: Vec<String>,
    /// Paths moved to the internal trash (informational, restorable from History).
    pub trashed: Vec<String>,
    /// Genuine failures only.
    pub errors: Vec<String>,
    pub restore_point_ok: bool,
    pub restore_point_error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportItem {
    pub path: String,
    pub item_type: String,
    pub confidence: u32,
    pub confidence_tier: String,
    pub size: Option<u64>,
}

#[tauri::command]
pub fn export_report_json(
    app_name: String,
    app_version: Option<String>,
    scan_result: ScanResult,
    delete_result: Option<DeleteResult>,
) -> Result<String, String> {
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

    let items: Vec<ExportItem> = scan_result
        .files
        .iter()
        .chain(scan_result.registry.iter())
        .chain(scan_result.services.iter())
        .chain(scan_result.startup.iter())
        .chain(scan_result.hosts.iter())
        .chain(scan_result.tasks.iter())
        .map(|item| ExportItem {
            path: item.path.clone(),
            item_type: format!("{:?}", item.item_type),
            confidence: item.confidence,
            confidence_tier: format!("{:?}", item.confidence_tier),
            size: item.size,
        })
        .collect();

    let report = ExportReport {
        app_name: app_name.clone(),
        app_version,
        timestamp,
        scan_summary: ScanSummary {
            total_files: scan_result.files.len(),
            total_registry: scan_result.registry.len(),
            total_services: scan_result.services.len(),
            total_startup: scan_result.startup.len(),
            total_hosts: scan_result.hosts.len(),
            total_tasks: scan_result.tasks.len(),
            total_items: items.len(),
        },
        delete_summary: delete_result.map(|dr| DeleteSummary {
            deleted: dr.deleted,
            skipped: dr.skipped,
            skipped_items: dr.skipped_items,
            already_gone: dr.already_gone,
            already_gone_paths: dr.already_gone_paths,
            trashed: dr.trashed,
            errors: dr.errors,
            restore_point_ok: dr.restore_point_ok,
            restore_point_error: dr.restore_point_error,
        }),
        items,
    };

    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;

    let reports_dir = get_reports_dir();
    fs::create_dir_all(&reports_dir).map_err(|e| e.to_string())?;

    let filename = format!(
        "clearout-{}-{}.json",
        sanitize_filename(&app_name),
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let filepath = reports_dir.join(&filename);

    fs::write(&filepath, &json).map_err(|e| e.to_string())?;

    Ok(filepath.to_string_lossy().to_string())
}

#[tauri::command]
pub fn export_report_txt(
    app_name: String,
    app_version: Option<String>,
    scan_result: ScanResult,
    delete_result: Option<DeleteResult>,
) -> Result<String, String> {
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let mut lines: Vec<String> = Vec::new();

    lines.push("ClearOut Scan Report".to_string());
    lines.push("=".repeat(40));
    lines.push(format!("App: {}", app_name));
    if let Some(ref ver) = app_version {
        lines.push(format!("Version: {}", ver));
    }
    lines.push(format!("Generated: {}", timestamp));
    lines.push("".to_string());

    lines.push("Scan Summary".to_string());
    lines.push("-".repeat(20));
    lines.push(format!("Files: {}", scan_result.files.len()));
    lines.push(format!("Registry: {}", scan_result.registry.len()));
    lines.push(format!("Services: {}", scan_result.services.len()));
    lines.push(format!("Startup: {}", scan_result.startup.len()));
    lines.push(format!("Hosts: {} (read-only)", scan_result.hosts.len()));
    lines.push(format!("Tasks: {} (read-only)", scan_result.tasks.len()));
    lines.push(format!(
        "Total: {}",
        scan_result.files.len()
            + scan_result.registry.len()
            + scan_result.services.len()
            + scan_result.startup.len()
            + scan_result.hosts.len()
            + scan_result.tasks.len()
    ));
    lines.push("".to_string());

    if let Some(ref dr) = delete_result {
        lines.push("Delete Summary".to_string());
        lines.push("-".repeat(20));
        lines.push(format!("Deleted: {}", dr.deleted));
        lines.push(format!("Skipped: {}", dr.skipped));
        for s in &dr.skipped_items {
            lines.push(format!("  - {} [{}]", s.path, s.reason));
        }
        if dr.already_gone > 0 {
            lines.push(format!("Already gone (nothing to do): {}", dr.already_gone));
            for p in &dr.already_gone_paths {
                lines.push(format!("  - {}", p));
            }
        }
        if dr.restore_point_ok {
            lines.push("Restore point: created".to_string());
        } else if let Some(ref rp) = dr.restore_point_error {
            lines.push(format!("Restore point: not created ({})", rp));
        }
        if !dr.trashed.is_empty() {
            lines.push("Trashed (restorable from History):".to_string());
            for t in &dr.trashed {
                lines.push(format!("  - {}", t));
            }
        }
        if !dr.errors.is_empty() {
            lines.push("Errors:".to_string());
            for err in &dr.errors {
                lines.push(format!("  - {}", err));
            }
        }
        lines.push("".to_string());
    }

    let all_items: Vec<_> = scan_result
        .files
        .iter()
        .chain(scan_result.registry.iter())
        .chain(scan_result.services.iter())
        .chain(scan_result.startup.iter())
        .chain(scan_result.hosts.iter())
        .chain(scan_result.tasks.iter())
        .collect();

    if !all_items.is_empty() {
        lines.push("Items Found".to_string());
        lines.push("-".repeat(20));
        for item in &all_items {
            lines.push(format!(
                "[{:?}] {} ({:?}, {} pts)",
                item.item_type,
                item.path,
                item.confidence_tier,
                item.confidence
            ));
        }
    }

    let content = lines.join("\n");

    let reports_dir = get_reports_dir();
    fs::create_dir_all(&reports_dir).map_err(|e| e.to_string())?;

    let filename = format!(
        "clearout-{}-{}.txt",
        sanitize_filename(&app_name),
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let filepath = reports_dir.join(&filename);

    fs::write(&filepath, &content).map_err(|e| e.to_string())?;

    Ok(filepath.to_string_lossy().to_string())
}

fn get_reports_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata)
            .join("ClearOut")
            .join("reports")
    } else {
        PathBuf::from(".")
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>()
        .to_lowercase()
}
