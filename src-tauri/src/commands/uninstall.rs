/// Exit codes that mean "uninstall actually happened".
fn is_success_exit(code: i32) -> bool {
    matches!(code, 0 | 3010)
}

/// Splits a Windows command line into (program, args) honoring double quotes.
/// Examples handled:
///   "C:\Program Files\App\uninstall.exe" /S
///   MsiExec.exe /X{1A2B...-GUID} /quiet
///   rundll32.exe setupapi.dll,InstallHinfSection ...
fn parse_command_line(cmdline: &str) -> (String, Vec<String>) {
    let trimmed = cmdline.trim();
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = trimmed.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                // Double quote inside quotes = escaped quote
                if in_quotes && chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ' ' | '\t' => {
                if in_quotes {
                    current.push(c);
                } else if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    if tokens.is_empty() {
        return (String::new(), Vec::new());
    }
    let program = tokens.remove(0);
    (program, tokens)
}

/// Extracts the MSI product code from tokens like ["/X{GUID}"], ["/X", "{GUID}"], ["/I{GUID}"].
fn extract_msi_product_code(tokens: &[String]) -> Option<String> {
    for t in tokens {
        let lower = t.to_lowercase();
        for flag in ["/x", "/i"] {
            if lower == flag || lower.starts_with(flag) {
                let rest = &t[flag.len()..];
                let rest = rest.trim_matches(|c| c == '"' || c == '=' || c == ' ');
                let rest_clean = if rest.is_empty() { None } else { Some(rest.to_string()) };
                if let Some(code) = rest_clean {
                    return Some(code.trim_matches('{').trim_matches('}').to_string());
                }
            }
        }
    }
    // GUID as a standalone token starting with '{'
    tokens
        .iter()
        .find(|t| {
            let t = t.trim_matches('"');
            t.starts_with('{') && t.contains('-') && t.ends_with('}')
        })
        .map(|t| t.trim_matches('"').trim_matches('{').trim_matches('}').to_string())
}

/// Builds (program, args) for a native uninstaller.
/// MSI uninstall strings are normalized to `msiexec.exe /x {CODE}` (interactive,
/// so the user sees the standard MSI uninstall UI).
fn build_uninstall_command(uninstall_string: &str) -> Result<(String, Vec<String>), String> {
    let (program, tokens) = parse_command_line(uninstall_string);
    if program.is_empty() {
        return Err("Empty uninstall string".to_string());
    }

    if program.to_lowercase().contains("msiexec") {
        let code = extract_msi_product_code(&tokens)
            .ok_or_else(|| format!("Could not parse MSI product code from: {}", uninstall_string))?;
        if code.is_empty() {
            return Err(format!("Could not parse MSI product code from: {}", uninstall_string));
        }
        return Ok(("msiexec.exe".to_string(), vec!["/x".to_string(), format!("{{{}}}", code)]));
    }

    Ok((program, tokens))
}

fn run_uninstall(uninstall_string: String) -> Result<i32, String> {
    let (program, args) = build_uninstall_command(&uninstall_string)?;

    let mut child = crate::commands::silent(&program)
        .args(&args)
        .spawn()
        .map_err(|e| format!("Failed to run uninstaller {}: {}", program, e))?;

    let exit_code = child.wait()
        .map_err(|e| format!("Failed waiting for uninstaller: {}", e))?
        .code()
        .unwrap_or(-1);

    // msiexec relaunches itself elevated via UAC consent; the parent process
    // exits early while the real uninstall keeps running. Poll until no
    // msiexec process remains (bounded) so callers don't scan mid-uninstall.
    if program.to_lowercase().contains("msiexec") && is_success_exit(exit_code) {
        for _ in 0..60 {
            if !msiexec_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    Ok(exit_code)
}

#[tauri::command]
pub async fn run_uninstaller(uninstall_string: String) -> Result<i32, String> {
    tokio::task::spawn_blocking(move || run_uninstall(uninstall_string))
        .await
        .map_err(|e| format!("Uninstall task failed: {}", e))?
}

fn msiexec_running() -> bool {
    let out = crate::commands::silent("tasklist.exe")
        .args(["/FI", "IMAGENAME eq msiexec.exe", "/NH"])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_lowercase().contains("msiexec.exe"),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_program_with_args() {
        let (prog, args) = parse_command_line(r#""C:\Program Files\App\uninstall.exe" /S --flag"#);
        assert_eq!(prog, r#"C:\Program Files\App\uninstall.exe"#);
        assert_eq!(args, vec!["/S", "--flag"]);
    }

    #[test]
    fn parses_unquoted_short_path() {
        let (prog, args) = parse_command_line(r"C:\unins.exe /silent");
        assert_eq!(prog, r"C:\unins.exe");
        assert_eq!(args, vec!["/silent"]);
    }

    #[test]
    fn parses_quoted_arg_containing_spaces() {
        let (prog, args) = parse_command_line(r#""C:\a.exe" "/x some value" /y"#);
        assert_eq!(prog, r"C:\a.exe");
        assert_eq!(args, vec![r"/x some value", "/y"]);
    }

    #[test]
    fn empty_and_whitespace_only() {
        assert_eq!(parse_command_line("").0, "");
        assert_eq!(parse_command_line("   ").0, "");
    }

    #[test]
    fn msiexec_guid_token_separate() {
        let cmd = build_uninstall_command(r#"MsiExec.exe /X{8B2C1A3D-0000-0000-0000-000000000000}"#).unwrap();
        assert_eq!(cmd.0, "msiexec.exe");
        assert_eq!(cmd.1, vec!["/x", "{8B2C1A3D-0000-0000-0000-000000000000}"]);
    }

    #[test]
    fn msiexec_guid_token_split() {
        let cmd = build_uninstall_command(r#""C:\Windows\System32\MsiExec.exe" /X {8B2C1A3D-0000-0000-0000-000000000000} /quiet"#).unwrap();
        assert_eq!(cmd.0, "msiexec.exe");
        assert_eq!(cmd.1, vec!["/x", "{8B2C1A3D-0000-0000-0000-000000000000}"]);
    }

    #[test]
    fn msiexec_missing_code_errors() {
        assert!(build_uninstall_command("MsiExec.exe /x").is_err());
    }

    #[test]
    fn non_msi_passthrough() {
        let cmd = build_uninstall_command(r#""C:\Program Files (x86)\App\unins000.exe" /SILENT /NORESTART"#).unwrap();
        assert_eq!(cmd.0, r"C:\Program Files (x86)\App\unins000.exe");
        assert_eq!(cmd.1, vec!["/SILENT", "/NORESTART"]);
    }

    #[test]
    fn success_exit_codes() {
        assert!(is_success_exit(0));
        assert!(is_success_exit(3010));
        assert!(!is_success_exit(1602));
        assert!(!is_success_exit(-1));
    }
}
