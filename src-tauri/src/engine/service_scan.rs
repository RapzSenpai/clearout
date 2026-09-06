use crate::engine::matching::{match_name_strict, AppSignals, MatchKind};
use crate::models::{AppInfo, LeftoverItem, LeftoverType};
use winreg::enums::*;
use winreg::RegKey;

/// Normalize a service ImagePath for comparison: strip surrounding quotes,
/// expand %SystemRoot% and the \\SystemRoot / \\??\\C: / bare System32 forms,
/// lowercase. Keeps the trailing arguments (-k netsvcs).
pub(crate) fn normalized_image_path(raw: &str) -> String {
    let mut p = raw
        .trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
        .to_lowercase();
    p = p.replace("%systemroot%", "c:\\windows");
    if let Some(rest) = p.strip_prefix("\\??\\") {
        p = rest.to_string();
    }
    if let Some(rest) = p.strip_prefix("\\systemroot") {
        p = format!("c:\\windows{}", rest);
    } else if let Some(rest) = p.strip_prefix("system32") {
        p = format!("c:\\windows\\{}", rest);
    }
    p
}

/// True when the (already-normalized) service binary lives inside Windows —
/// such services are system-owned and must never be treated as app leftovers.
pub(crate) fn is_system_service_path(normalized: &str) -> bool {
    normalized.starts_with("c:\\windows")
        || normalized.starts_with("c:\\program files\\windows")
        // Defender Platform lives under ProgramData but is Windows-owned.
        || normalized.starts_with("c:\\programdata\\microsoft\\windows")
}

pub fn scan_services(app: &AppInfo) -> Result<Vec<LeftoverItem>, String> {
    let mut items: Vec<LeftoverItem> = Vec::new();
    let sig = AppSignals::from_app(app);

    let service_key = match RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags("SYSTEM\\CurrentControlSet\\Services", KEY_READ) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("[scan] services key open failed (non-admin?): {}", e);
            return Ok(items);
        }
    };

    for subkey_name in service_key.enum_keys().filter_map(|k| k.ok()) {
        let subkey = match service_key.open_subkey_with_flags(&subkey_name, KEY_READ) {
            Ok(k) => k,
            Err(_) => continue,
        };

        let display_name: String = subkey
            .get_value("DisplayName")
            .unwrap_or_default();

        if display_name.is_empty() {
            continue;
        }

        // Skip Windows system services outright: their display names often
        // contain generic words that collide with app names (e.g. "core" in
        // CoreMessaging / Microsoft Security Core). The binary path is the
        // authoritative owner check.
        let image_path: String = subkey.get_value("ImagePath").unwrap_or_default();
        if image_path.trim().is_empty() {
            continue;
        }
        let image_lower = normalized_image_path(&image_path);
        if is_system_service_path(&image_lower) {
            continue;
        }

        let display_lower = display_name.to_lowercase();
        // Strict matching: services are sensitive — a lone keyword hit on the
        // display name or binary path is never enough, full name required.
        let matched = match_name_strict(&sig, &display_lower) != MatchKind::None
            || match_name_strict(&sig, &image_lower) != MatchKind::None;

        if matched {
            let full_path = format!("Service: {} ({})", subkey_name, display_name);
                items.push(LeftoverItem {
                    id: crate::models::make_leftover_id(&LeftoverType::Service, &full_path),
                    path: full_path,
                item_type: LeftoverType::Service,
                confidence: 0,
                confidence_tier: crate::models::ConfidenceTier::Low,
                associated_app: app.name.clone(),
                size: None,
            });
        }
    }

    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_image_path_is_system() {
        assert!(is_system_service_path(&normalized_image_path(
            r#""C:\Windows\System32\svchost.exe" -k netsvcs -p"#
        )));
        assert!(is_system_service_path(&normalized_image_path(
            r#"%SystemRoot%\System32\svchost.exe -k LocalServiceNetworkRestricted"#
        )));
        assert!(is_system_service_path(&normalized_image_path(
            r#"\SystemRoot\System32\svchost.exe"#
        )));
    }

    #[test]
    fn app_image_path_is_not_system() {
        assert!(!is_system_service_path(&normalized_image_path(
            r#""C:\Program Files\Core Temp\CoreTempService.exe" -run"#
        )));
        assert!(!is_system_service_path(&normalized_image_path(
            r#"C:\Users\bob\AppData\Local\MyApp\svc.exe"#
        )));
    }

    #[test]
    fn quotes_and_case_normalized() {
        let n = normalized_image_path(r#""C:\PROGRAM FILES\FOO\bar.exe" -k x"#);
        assert!(n.starts_with("c:\\program files\\foo\\bar.exe"));
        assert!(!n.starts_with('"'));
    }

    #[test]
    fn kernel_driver_root_forms_normalized() {
        let n = normalized_image_path(r"\SystemRoot\System32\drivers\cpuz154.sys");
        assert!(is_system_service_path(&n));
        let n = normalized_image_path(r"\??\C:\Windows\System32\drivers\x.sys");
        assert!(is_system_service_path(&n));
        let n = normalized_image_path(r"System32\DRIVERS\ndis.sys");
        assert!(is_system_service_path(&n));
    }

    #[test]
    fn defender_platform_is_system() {
        let n = normalized_image_path(
            r#""C:\ProgramData\Microsoft\Windows Defender\Platform\4.18.26080.3-0\MpDefenderCoreService.exe""#,
        );
        assert!(is_system_service_path(&n));
    }
}