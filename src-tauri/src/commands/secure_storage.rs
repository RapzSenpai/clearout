use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

fn target_name(provider: &str) -> String {
    format!("ClearOut:api:{}", provider.trim().to_lowercase())
}

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

#[tauri::command]
pub fn save_api_key(provider: String, api_key: String) -> Result<(), String> {
    if provider.trim().is_empty() {
        return Err("Provider required".to_string());
    }
    if api_key.trim().is_empty() {
        return delete_api_key(provider);
    }
    #[cfg(windows)]
    {
        use windows::Win32::Security::Credentials::{
            CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        };
        let target = target_name(&provider);
        let mut target_w = to_wide(&target);
        let secret_w = to_wide(&api_key);
        // ponytail: Credential Manager beats keychain crate; ceiling is Windows-only, upgrade is cross-platform keyring.
        let cred = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: windows::core::PWSTR(target_w.as_mut_ptr()),
            CredentialBlobSize: ((secret_w.len().saturating_sub(1)) * 2) as u32,
            CredentialBlob: secret_w.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        unsafe { CredWriteW(&cred, 0).map_err(|e| format!("Failed to save key: {}", e))?; }
        crate::commands::log_event("info", &format!("api key saved provider={}", provider));
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (provider, api_key);
        Err("Secure storage is Windows-only".to_string())
    }
}

pub(crate) fn load_api_key_internal(provider: &str) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Security::Credentials::{CredReadW, CredFree, CRED_TYPE_GENERIC};
        let target = target_name(provider);
        let target_w = to_wide(&target);
        let mut out = None;
        unsafe {
            let mut cred_ptr = std::ptr::null_mut();
            if CredReadW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, 0, &mut cred_ptr).is_ok()
                && !cred_ptr.is_null()
            {
                let cred = &*cred_ptr;
                let bytes = std::slice::from_raw_parts(
                    cred.CredentialBlob,
                    cred.CredentialBlobSize as usize,
                );
                // Stored as UTF-16 bytes; decode lossily.
                let u16len = bytes.len() / 2;
                let wide: Vec<u16> = (0..u16len)
                    .map(|i| u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]))
                    .collect();
                out = Some(String::from_utf16_lossy(&wide));
                CredFree(cred_ptr as *const std::ffi::c_void);
            }
        }
        out.filter(|s| !s.is_empty())
    }
    #[cfg(not(windows))]
    {
        let _ = provider;
        None
    }
}

#[tauri::command]
pub fn api_key_status(provider: String) -> Result<bool, String> {
    Ok(load_api_key_internal(&provider).is_some())
}

#[tauri::command]
pub fn delete_api_key(provider: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
        let target = target_name(&provider);
        let target_w = to_wide(&target);
        unsafe {
            // Missing credential is not an error for delete UX.
            let _ = CredDeleteW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, 0);
        }
        crate::commands::log_event("info", &format!("api key deleted provider={}", provider));
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = provider;
        Err("Secure storage is Windows-only".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_names_scoped() {
        assert_eq!(target_name("Groq"), "ClearOut:api:groq");
        assert_eq!(target_name(" custom "), "ClearOut:api:custom");
    }
}
