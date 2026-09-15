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
/// Random UUIDs per scan broke rescan diffing and multi-app dedupe.
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
pub struct AttentionItem {
    pub path: String,
    pub reason: String,
    pub action: String,
    pub status: String,
    pub item_type: String,
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
    /// Items requiring user attention: locked, access-denied, scheduled for
    /// reboot, or otherwise not removed. Successes are not listed here so the
    /// results table stays clean and scannable.
    pub attention_items: Vec<AttentionItem>,
    /// Ids of items successfully removed/deleted — used for verification scans.
    pub deleted_ids: Vec<String>,
    /// Genuine failures only (top-level summaries; details live in attention_items).
    pub errors: Vec<String>,
    /// Restore-point outcome (only meaningful when creation was requested):
    /// true when the checkpoint was created. Kept separate from `errors` so a
    /// skipped/failed restore point never reads as a failed deletion.
    pub restore_point_ok: bool,
    pub restore_point_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub assessment: String,
    pub confidence: String,
    pub recommendation: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leftover_id_stable_rescan() {
        // ponytail: stable id beats UUID; ceiling is case-insensitive path match, upgrade is canonical path.
        let a = make_leftover_id(&LeftoverType::File, r"C:\App\X.DLL");
        let b = make_leftover_id(&LeftoverType::File, r"c:\app\x.dll");
        assert_eq!(a, b);
        let c = make_leftover_id(&LeftoverType::Folder, r"c:\app\x.dll");
        assert_ne!(a, c);
    }
}
