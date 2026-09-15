const TASK_NAME: &str = "ClearOut Weekly Scan";

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SchedulerStatus {
    pub enabled: bool,
    pub time: Option<String>,
    pub next_run: Option<String>,
}

#[tauri::command]
pub fn get_scheduler_status() -> Result<SchedulerStatus, String> {
    let out = crate::commands::silent("schtasks")
        .args(["/Query", "/TN", TASK_NAME, "/FO", "LIST", "/V"])
        .output()
        .map_err(|e| e.to_string())?;

    if !out.status.success() {
        return Ok(SchedulerStatus { enabled: false, time: None, next_run: None });
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut time: Option<String> = None;
    let mut next_run: Option<String> = None;
    let mut enabled = false;
    for line in text.lines() {
        let l = line.trim();
        if l.to_lowercase().starts_with("start time:") {
            let value = l.split_once(':').map(|(_, s)| s.trim()).unwrap_or("");
            time = parse_schtasks_hhmm(value);
        }
        if l.to_lowercase().starts_with("next run time:") {
            next_run = l.split_once(':').map(|(_, s)| s.trim().to_string());
        }
        // "Status: Ready" is the only enabled signal. A disabled task still
        // contains the task name, so name-matching here lied about the state.
        if l.to_lowercase().starts_with("status:") && l.to_lowercase().contains("ready") {
            enabled = true;
        }
    }
    Ok(SchedulerStatus { enabled, time, next_run })
}

/// Extract HH:MM from schtasks time text across locales: `14:30:00`,
/// `2:00:00 PM`, `14:30`. Returns None when no clock token found.
fn parse_schtasks_hhmm(value: &str) -> Option<String> {
    let toks: Vec<&str> = value.split_whitespace().collect();
    for (i, tok) in toks.iter().enumerate() {
        let clean = tok.trim_matches(|c| c == ',' || c == '.');
        let mut it = clean.split(':');
        let (h, m) = match (it.next(), it.next()) {
            (Some(h), Some(m)) if !h.is_empty() && h.len() <= 2 && m.len() >= 2 => {
                let m = &m[..2];
                if !h.chars().all(|c| c.is_ascii_digit()) || !m.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                (h, m)
            }
            _ => continue,
        };
        let mut hour: u32 = h.parse().ok()?;
        let min: u32 = m.parse().ok()?;
        if min >= 60 || hour > 23 {
            continue;
        }
        // 12-hour locales append AM/PM after the time token.
        if let Some(marker) = toks.get(i + 1).map(|w| w.to_lowercase()) {
            if marker.starts_with('p') && hour < 12 {
                hour += 12;
            } else if marker.starts_with('a') && hour == 12 {
                hour = 0;
            }
        }
        return Some(format!("{:02}:{}", hour, m));
    }
    None
}

#[tauri::command]
pub fn set_scheduler(enabled: bool, time: Option<String>) -> Result<(), String> {
    if !enabled {
        let _ = crate::commands::silent("schtasks")
            .args(["/Delete", "/TN", TASK_NAME, "/F"])
            .output();
        return Ok(());
    }
    let t = time.unwrap_or_else(|| "02:00".to_string());
    // Validate HH:MM before handing it to schtasks — an invalid /ST makes
    // /Create fail and silently leaves the schedule disabled.
    let parts: Vec<&str> = t.split(':').collect();
    let valid = parts.len() == 2
        && parts[0].len() == 2
        && parts[1].len() == 2
        && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit()))
        && parts[0].parse::<u32>().map(|h| h < 24).unwrap_or(false)
        && parts[1].parse::<u32>().map(|m| m < 60).unwrap_or(false);
    if !valid {
        return Err(format!("Invalid schedule time '{}' (expected HH:MM)", t));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_str = exe.to_string_lossy().to_string();

    // Delete existing first
    let _ = crate::commands::silent("schtasks")
        .args(["/Delete", "/TN", TASK_NAME, "/F"])
        .output();

    let out = crate::commands::silent("schtasks")
        .args([
            "/Create",
            "/TN", TASK_NAME,
            "/TR", &format!("\"{}\"", exe_str),
            "/SC", "WEEKLY",
            "/D", "SUN",
            "/ST", &t,
            "/F",
        ])
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        let out2 = String::from_utf8_lossy(&out.stdout).to_string();
        Err(format!("schtasks failed: {} {}", err, out2))
    }
}

#[tauri::command]
pub fn is_admin() -> bool {
    // Try to open HKLM\SOFTWARE with write — crude but avoids winapi
    use winreg::enums::*;
    use winreg::RegKey;
    RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags("SOFTWARE", KEY_WRITE)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_24h_and_12h_times() {
        assert_eq!(parse_schtasks_hhmm("2026/09/15 14:30:00").as_deref(), Some("14:30"));
        assert_eq!(parse_schtasks_hhmm("9/15/2026 2:00:00 PM").as_deref(), Some("14:00"));
        assert_eq!(parse_schtasks_hhmm("9/15/2026 12:00:00 AM").as_deref(), Some("00:00"));
        assert_eq!(parse_schtasks_hhmm("14:30").as_deref(), Some("14:30"));
        assert_eq!(parse_schtasks_hhmm("N/A"), None);
    }
}
