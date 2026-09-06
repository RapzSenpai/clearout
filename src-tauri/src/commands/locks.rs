use crate::models::LockInfo;

#[tauri::command]
pub fn check_locks(path: String) -> Result<Vec<LockInfo>, String> {
    let mut locks: Vec<LockInfo> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_BACKUP_SEMANTICS,
            OPEN_EXISTING,
        };
        use windows::Win32::Foundation::{CloseHandle, GENERIC_READ};
        use windows::core::PCWSTR;

        let path_wide = windows::core::HSTRING::from(&path);
        let pcwstr = PCWSTR::from_raw(path_wide.as_ptr());

        unsafe {
            let handle = CreateFileW(
                pcwstr,
                GENERIC_READ.0,
                windows::Win32::Storage::FileSystem::FILE_SHARE_MODE(0x07),
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS,
                None,
            );

            match handle {
                Ok(h) => {
                    let _ = CloseHandle(h);
                }
                Err(e) => {
                    // FILE_NOT_FOUND / PATH_NOT_FOUND mean the item is gone,
                    // not locked — reporting them as locks misled the user
                    // before deletion.
                    let code = e.code().0 as u32;
                    let missing = code == 0x8007_0002 || code == 0x8007_0003;
                    if !missing {
                        locks.push(LockInfo {
                            pid: 0,
                            process_name: "Unknown".to_string(),
                            path,
                        });
                    }
                }
            }
        }
    }

    Ok(locks)
}
