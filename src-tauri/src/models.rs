use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub publisher: Option<String>,
    pub install_location: Option<String>,
    pub estimated_size: Option<u64>,
    pub uninstall_string: Option<String>,
    pub install_date: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeftoverItem {
    pub id: String,
    pub path: String,
    pub item_type: LeftoverType,
    pub confidence: u32,
    pub confidence_tier: ConfidenceTier,
    pub associated_app: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LeftoverType {
    File,
    Folder,
    Registry,
    Service,
    Startup,
    Hosts,
    Task,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfidenceTier {
    High,
    Medium,
    Low,
}

/// Deterministic id: stable across scans for the same path/type.
/// Random UUIDs per scan broke cross-scan diffing (verify_scan) and multi-app dedupe.
pub fn make_leftover_id(item_type: &LeftoverType, path: &str) -> String {
    format!("{:?}|{}", item_type, path.to_lowercase())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub files: Vec<LeftoverItem>,
    pub registry: Vec<LeftoverItem>,
    pub services: Vec<LeftoverItem>,
    pub startup: Vec<LeftoverItem>,
    pub hosts: Vec<LeftoverItem>,
    pub tasks: Vec<LeftoverItem>,
}

/// One skipped item plus the exact rule that caused the skip, so the UI can
/// explain "1 skipped" instead of making the user guess.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedItem {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteResult {
    pub deleted: usize,
    pub skipped: usize,
    /// Every rule-skipped item with the rule that caused the skip.
    pub skipped_items: Vec<SkippedItem>,
    /// Selected items that no longer existed at delete time (e.g. the native
    /// uninstaller removed them between scan and delete). Not a skip — there
    /// was simply nothing left to do.
    pub already_gone: usize,
    pub already_gone_paths: Vec<String>,
    /// Paths of items successfully moved to the internal trash (informational).
    pub trashed: Vec<String>,
    /// Ids of items successfully removed/deleted — used for verification scans.
    pub deleted_ids: Vec<String>,
    /// Genuine failures only.
    pub errors: Vec<String>,
    /// Restore-point outcome (only meaningful when creation was requested):
    /// true when the checkpoint was created. Kept separate from `errors` so a
    /// skipped/failed restore point never reads as a failed deletion.
    pub restore_point_ok: bool,
    pub restore_point_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    pub process_name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub assessment: String,
    pub confidence: String,
    pub recommendation: String,
}
