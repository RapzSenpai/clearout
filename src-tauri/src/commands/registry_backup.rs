use crate::engine::registry_backup as rb;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryBackupEntry {
    pub id: String,
    pub created: String,
    pub root_path: String,
    pub key_count: usize,
    pub value_count: usize,
}

#[tauri::command]
pub fn list_registry_backups() -> Result<Vec<RegistryBackupEntry>, String> {
    let backups = rb::list_backups()?;
    Ok(backups
        .into_iter()
        .map(|b| RegistryBackupEntry {
            id: b.id,
            created: b.created,
            root_path: b.root_path,
            key_count: b.keys.len(),
            value_count: b.keys.iter().map(|k| k.values.len()).sum(),
        })
        .collect())
}

#[tauri::command]
pub fn restore_registry_backup(id: String) -> Result<(), String> {
    rb::restore_backup(&id)
}

#[tauri::command]
pub fn delete_registry_backup(id: String) -> Result<(), String> {
    rb::delete_backup(&id)
}

/// Delete every registry backup file. These backups are the only way to
/// restore removed registry keys, so the UI confirms with typed confirmation
/// before calling this.
#[tauri::command]
pub fn clear_registry_backups() -> Result<u32, String> {
    rb::clear_backups()
}
