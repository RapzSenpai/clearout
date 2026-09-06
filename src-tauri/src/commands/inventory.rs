use crate::models::AppInfo;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use winreg::enums::*;
use winreg::RegKey;

use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Shell::{ExtractIconExW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL, HICON};

#[tauri::command]
pub async fn get_installed_apps() -> Result<Vec<AppInfo>, String> {
    tokio::task::spawn_blocking(run_inventory)
        .await
        .map_err(|e| format!("Inventory task failed: {}", e))?
}

fn run_inventory() -> Result<Vec<AppInfo>, String> {
    let mut apps: Vec<AppInfo> = Vec::new();
    let mut seen: HashMap<String, bool> = HashMap::new();

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    let paths = vec![
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    ];

    for base_path in &paths {
        if let Ok(key) = hklm.open_subkey_with_flags(base_path, KEY_READ) {
            for subkey_name in key.enum_keys().filter_map(|k| k.ok()) {
                if let Ok(subkey) = key.open_subkey_with_flags(&subkey_name, KEY_READ) {
                    if let Some(app) = parse_app_info(&subkey, &subkey_name) {
                        if !app.name.is_empty() && !seen.contains_key(&app.id) {
                            seen.insert(app.id.clone(), true);
                            apps.push(app);
                        }
                    }
                }
            }
        }

        if let Ok(key) = hkcu.open_subkey_with_flags(base_path, KEY_READ) {
            for subkey_name in key.enum_keys().filter_map(|k| k.ok()) {
                if let Ok(subkey) = key.open_subkey_with_flags(&subkey_name, KEY_READ) {
                    if let Some(app) = parse_app_info(&subkey, &subkey_name) {
                        if !app.name.is_empty() && !seen.contains_key(&app.id) {
                            seen.insert(app.id.clone(), true);
                            apps.push(app);
                        }
                    }
                }
            }
        }
    }

    apps.sort_by_key(|a| a.name.to_lowercase());
    Ok(apps)
}

fn parse_app_info(subkey: &RegKey, subkey_name: &str) -> Option<AppInfo> {
    let name: String = subkey.get_value("DisplayName").ok()?;
    if name.is_empty() {
        return None;
    }

    let version: Option<String> = subkey.get_value("DisplayVersion").ok();
    let publisher: Option<String> = subkey.get_value("Publisher").ok();
    let install_location: Option<String> = subkey.get_value("InstallLocation").ok();
    let estimated_size: Option<u64> = subkey
        .get_value::<u32, _>("EstimatedSize")
        .ok()
        .map(|s| s as u64 * 1024);
    let uninstall_string: Option<String> = subkey.get_value("UninstallString").ok();
    let install_date: Option<String> = subkey.get_value("InstallDate").ok();

    let icon = extract_icon(subkey, install_location.as_deref(), uninstall_string.as_deref());

    Some(AppInfo {
        id: subkey_name.to_string(),
        name,
        version,
        publisher,
        install_location,
        estimated_size,
        uninstall_string,
        install_date,
        icon,
    })
}

fn extract_icon(
    subkey: &RegKey,
    install_location: Option<&str>,
    uninstall_string: Option<&str>,
) -> Option<String> {
    if let Ok(display_icon) = subkey.get_value::<String, _>("DisplayIcon") {
        let trimmed = display_icon.trim();
        if !trimmed.is_empty() {
            let (path, index) = parse_display_icon(trimmed);
            if let Some(icon) = extract_icon_from_path(&path, index) {
                return Some(icon);
            }
        }
    }

    // Fallback 1: Check InstallLocation directory for .exe or .ico
    if let Some(loc) = install_location {
        let loc_clean = loc.trim().trim_matches('"');
        if !loc_clean.is_empty() && Path::new(loc_clean).exists() {
            if let Some(icon) = find_icon_in_dir(loc_clean) {
                return Some(icon);
            }
        }
    }

    // Fallback 2: Check UninstallString executable
    if let Some(uninst) = uninstall_string {
        let exe = extract_exe_path(uninst);
        if !exe.is_empty() && Path::new(&exe).exists() {
            if let Some(icon) = extract_icon_from_path(&exe, 0) {
                return Some(icon);
            }
        }
    }

    None
}

fn parse_display_icon(display_icon: &str) -> (String, i32) {
    let trimmed = display_icon.trim();
    if let Some(comma_pos) = trimmed.rfind(',') {
        let path_part = trimmed[..comma_pos].trim().trim_matches('"').to_string();
        let index_str = trimmed[comma_pos + 1..].trim();
        let index: i32 = index_str.parse().unwrap_or(0);
        if Path::new(&path_part).exists() || path_part.contains('\\') {
            return (path_part, index);
        }
    }
    (trimmed.trim_matches('"').to_string(), 0)
}

fn extract_exe_path(cmd: &str) -> String {
    let trimmed = cmd.trim();
    if let Some(stripped) = trimmed.strip_prefix('"') {
        if let Some(end) = stripped.find('"') {
            return stripped[..end].to_string();
        }
    }
    trimmed.split_whitespace().next().unwrap_or("").to_string()
}

fn find_icon_in_dir(dir_path: &str) -> Option<String> {
    let path = Path::new(dir_path);
    if !path.is_dir() {
        return None;
    }

    if let Ok(entries) = std::fs::read_dir(path) {
        let mut exes = Vec::new();
        let mut icos = Vec::new();

        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_file() {
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    if ext_lower == "ico" {
                        icos.push(p);
                    } else if ext_lower == "exe" {
                        exes.push(p);
                    }
                }
            }
        }

        // Prioritize .ico then .exe
        for ico in icos {
            if let Some(icon) = extract_icon_from_path(&ico.to_string_lossy(), 0) {
                return Some(icon);
            }
        }

        for exe in exes {
            let exe_str = exe.to_string_lossy();
            let lower = exe_str.to_lowercase();
            // Skip common uninstall/update binaries when looking for main app icon
            if !lower.contains("unins") && !lower.contains("update") && !lower.contains("setup") {
                if let Some(icon) = extract_icon_from_path(&exe_str, 0) {
                    return Some(icon);
                }
            }
        }
    }

    None
}

fn wide_string(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn extract_icon_from_path(path: &str, index: i32) -> Option<String> {
    let clean_path = path.trim().trim_matches('"');
    if clean_path.is_empty() || !Path::new(clean_path).exists() {
        return None;
    }

    let wide_path = wide_string(clean_path);

    unsafe {
        let mut hicon: HICON = std::mem::zeroed();
        let count = ExtractIconExW(
            windows::core::PCWSTR::from_raw(wide_path.as_ptr()),
            index,
            Some(&mut hicon),
            None,
            1,
        );

        if count > 0 && !hicon.is_invalid() {
            let result = icon_to_png_base64(hicon);
            DestroyIcon(hicon).ok();
            if result.is_some() {
                return result;
            }
        }

        // Fallback: SHGetFileInfoW for shell icon
        let mut shfi: SHFILEINFOW = std::mem::zeroed();
        let res = SHGetFileInfoW(
            windows::core::PCWSTR::from_raw(wide_path.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );

        if res != 0 && !shfi.hIcon.is_invalid() {
            let result = icon_to_png_base64(shfi.hIcon);
            DestroyIcon(shfi.hIcon).ok();
            return result;
        }
    }

    None
}

unsafe fn icon_to_png_base64(hicon: HICON) -> Option<String> {
    let size: i32 = 32;
    let hdc = CreateCompatibleDC(None);
    if hdc.is_invalid() {
        return None;
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: (size * size * 4) as u32,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
    };

    let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    let hbitmap = CreateDIBSection(
        hdc,
        &bmi,
        DIB_RGB_COLORS,
        &mut bits_ptr,
        None,
        0,
    ).ok();

    let hbitmap = match hbitmap {
        Some(h) if !h.is_invalid() => h,
        _ => {
            let _ = DeleteDC(hdc);
            return None;
        }
    };

    let old_bmp = SelectObject(hdc, HGDIOBJ(hbitmap.0 as _));

    let draw_ok = DrawIconEx(
        hdc,
        0,
        0,
        hicon,
        size,
        size,
        0,
        None,
        DI_NORMAL,
    ).is_ok();

    SelectObject(hdc, old_bmp);
    let _ = DeleteDC(hdc);

    if !draw_ok || bits_ptr.is_null() {
        let _ = DeleteObject(HGDIOBJ(hbitmap.0 as _));
        return None;
    }

    let total_pixels = (size * size) as usize;
    let raw_slice = std::slice::from_raw_parts(bits_ptr as *const u8, total_pixels * 4);

    let mut has_non_zero_alpha = false;
    for i in 0..total_pixels {
        if raw_slice[i * 4 + 3] > 0 {
            has_non_zero_alpha = true;
            break;
        }
    }

    let mut rgba = Vec::with_capacity(total_pixels * 4);
    for i in 0..total_pixels {
        let b = raw_slice[i * 4];
        let g = raw_slice[i * 4 + 1];
        let r = raw_slice[i * 4 + 2];
        let a = if has_non_zero_alpha {
            raw_slice[i * 4 + 3]
        } else {
            if r == 0 && g == 0 && b == 0 { 0 } else { 255 }
        };
        rgba.extend_from_slice(&[r, g, b, a]);
    }

    let _ = DeleteObject(HGDIOBJ(hbitmap.0 as _));

    let img = image::RgbaImage::from_raw(size as u32, size as u32, rgba)?;
    let mut png_bytes: Vec<u8> = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_bytes);
    img.write_to(&mut cursor, image::ImageFormat::Png).ok()?;

    use base64::Engine;
    Some(base64::engine::general_purpose::STANDARD.encode(&png_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_icon_with_comma_index() {
        let (path, index) = parse_display_icon(r#"C:\Program Files\App\app.exe,0"#);
        assert_eq!(path, r"C:\Program Files\App\app.exe");
        assert_eq!(index, 0);
    }

    #[test]
    fn display_icon_with_quotes_and_index() {
        let (path, index) = parse_display_icon(r#""C:\Program Files\App\app.exe",-2"#);
        assert_eq!(path, r"C:\Program Files\App\app.exe");
        assert_eq!(index, -2);
    }

    #[test]
    fn display_icon_bare_path() {
        let (path, index) = parse_display_icon(r"C:\some.ico");
        assert_eq!(path, r"C:\some.ico");
        assert_eq!(index, 0);
    }

    #[test]
    fn display_icon_index_unparseable_defaults_zero() {
        let (path, index) = parse_display_icon(r"C:\a.exe,abc");
        assert_eq!(path, r"C:\a.exe");
        assert_eq!(index, 0);
    }

    #[test]
    fn uninstall_exe_quoted_path() {
        assert_eq!(
            extract_exe_path(r#""C:\Program Files\App\unins000.exe" /SILENT"#),
            r"C:\Program Files\App\unins000.exe"
        );
    }

    #[test]
    fn uninstall_exe_unquoted_path() {
        assert_eq!(
            extract_exe_path(r"C:\unins.exe /SILENT"),
            r"C:\unins.exe"
        );
    }

    #[test]
    fn uninstall_exe_empty() {
        assert_eq!(extract_exe_path(""), "");
    }
}

