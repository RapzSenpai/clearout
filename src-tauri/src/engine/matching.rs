//! Shared leftover-name matching used by every scanner.
//!
//! Raw substring containment is banned on purpose: the word "core" from an app
//! called "Core Temp" must never match the Windows subsystem folder "OneCore".
//! All matching goes through tokens (whole words split on non-alphanumerics)
//! and a compacted full-name form, so partial-word collisions cannot happen.

use crate::models::AppInfo;

/// Words so common in Windows and software naming that matching them alone —
/// or even as one of two keyword hits — would flag unrelated system files.
/// An app whose keywords are ALL generic can only be matched by its full name.
pub const GENERIC_TOKENS: &[&str] = &[
    "app", "bin", "build", "cache", "clean", "cleaner", "com", "common", "config", "core",
    "corp", "data", "default", "files", "inc", "local", "log", "logs", "ltd", "llc",
    "manager", "microsoft", "ms", "onecore", "program", "roaming", "service", "services",
    "settings", "setup", "soft", "software", "system", "task", "tasks", "temp", "the",
    "tool", "tools", "tmp", "uninstall", "update", "updater", "utils", "version", "win",
    "windows", "x64", "x86", "client", "support", "driver", "drivers", "installer",
];

/// Signals extracted once per app, reused by all scanners.
pub struct AppSignals {
    /// Lowercased full display name, e.g. "core temp".
    pub name_lower: String,
    /// Alphanumeric-only name, e.g. "coretemp".
    pub compact: String,
    /// Lowercased package/registry id.
    pub id_lower: String,
    /// Lowercased publisher.
    pub publisher_lower: String,
    /// Distinctive name tokens (generic words removed).
    pub distinctive: Vec<String>,
    /// Distinctive publisher tokens.
    pub publisher_tokens: Vec<String>,
}

/// How strongly a candidate matches the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    None,
    /// Keyword-level hit — reviewable, low confidence.
    Weak,
    /// Full-name level hit — high confidence.
    Strong,
}

impl AppSignals {
    pub fn from_app(app: &AppInfo) -> Self {
        Self::from_parts(&app.name, &app.id, app.publisher.as_deref())
    }

    pub fn from_parts(name: &str, id: &str, publisher: Option<&str>) -> Self {
        let name_lower = name.to_lowercase().trim().to_string();
        let compact: String = name_lower.chars().filter(|c| c.is_alphanumeric()).collect();
        let keywords: Vec<String> = name_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 3)
            .map(|w| w.to_string())
            .collect();
        let distinctive: Vec<String> = keywords
            .iter()
            .filter(|w| !is_generic_token(w))
            .cloned()
            .collect();
        let publisher_lower = publisher.unwrap_or("").to_lowercase().trim().to_string();
        let publisher_tokens = publisher_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 4 && !is_generic_token(w))
            .map(|w| w.to_string())
            .collect();AppSignals {
            name_lower,
            compact,
            id_lower: id.to_lowercase(),
            publisher_lower,
            distinctive,
            publisher_tokens,
        }
    }
}

fn is_generic_token(tok: &str) -> bool {
    GENERIC_TOKENS.contains(&tok)
}

fn tokens_of(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

fn compact_of(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Match a candidate NAME (file name, registry key, task name, service name).
fn match_name_impl(sig: &AppSignals, target_lower: &str, allow_single_keyword: bool) -> MatchKind {
    let t = target_lower.trim();
    if t.is_empty() {
        return MatchKind::None;
    }

    // Full name appears verbatim inside the candidate ("core temp" in
    // "core temp uninstall.exe"). Strongest signal.
    if !sig.name_lower.is_empty() && t.contains(&sig.name_lower) {
        return MatchKind::Strong;
    }

    // Compact full name appears inside the compacted candidate
    // ("coretemp" in "coretemp.exe", "core-temp" in "core_temp.dll").
    // Needs 5+ chars so short names can't ride inside random words.
    if sig.compact.len() >= 5 && compact_of(t).contains(&sig.compact) {
        return MatchKind::Strong;
    }

    // Package id — same containment rule as the compact name.
    if sig.id_lower.len() >= 5 && (t.contains(&sig.id_lower) || compact_of(t).contains(&sig.id_lower)) {
        return MatchKind::Strong;
    }

    // Keyword path: whole-token equality only, distinctive keywords only.
    // "core" as a token can match; "core" inside "onecore" can never.
    let target_tokens = tokens_of(t);
    let matched: Vec<&String> = target_tokens
        .iter()
        .filter(|tok| sig.distinctive.iter().any(|k| k == *tok))
        .collect();
    let hits = matched.len();
    let single_ok = allow_single_keyword
        && hits == 1
        && matched[0].len() >= 5;
    if hits >= 2 || single_ok {
        return MatchKind::Weak;
    }

    // Publisher tokens, same token rule (used for vendor folder heuristics
    // by individual scanners at lower confidence).
    if !sig.publisher_tokens.is_empty()
        && target_tokens
            .iter()
            .any(|tok| sig.publisher_tokens.iter().any(|k| k == tok))
    {
        return MatchKind::Weak;
    }

    MatchKind::None
}

/// Match a candidate name where a single distinctive keyword of 5+ chars is
/// enough (file/folder names are short and human-curated).
pub fn match_name(sig: &AppSignals, target_lower: &str) -> MatchKind {
    match_name_impl(sig, target_lower, true)
}

/// Match a strict candidate (services, startup entries): a single keyword hit
/// is NOT enough — needs the full name or 2+ distinctive tokens.
pub fn match_name_strict(sig: &AppSignals, target_lower: &str) -> MatchKind {
    match_name_impl(sig, target_lower, false)
}

/// Match free TEXT (task XML content, registry value data, hosts lines).
/// Full name or compact name containment, or 2+ distinctive tokens.
/// Single keywords are never enough — file contents throw too many false hits.
pub fn match_text(sig: &AppSignals, text_lower: &str) -> MatchKind {
    if text_lower.contains(&sig.name_lower) && !sig.name_lower.is_empty() {
        return MatchKind::Strong;
    }
    if sig.compact.len() >= 5 && text_lower.contains(&sig.compact) {
        return MatchKind::Strong;
    }
    if sig.id_lower.len() >= 5 && text_lower.contains(&sig.id_lower) {
        return MatchKind::Strong;
    }
    let target_tokens = tokens_of(text_lower);
    let hits = target_tokens
        .iter()
        .filter(|tok| sig.distinctive.iter().any(|k| k == *tok))
        .count();
    if hits >= 2 {
        return MatchKind::Weak;
    }
    MatchKind::None
}

/// Windows-owned locations that must never be scanned or flagged as file
/// leftovers, no matter what the name matching says. Scanners that look
/// under System32 by design (tasks, hosts) enforce their own ownership
/// rules instead.
pub fn is_windows_owned_path(path_lower: &str) -> bool {
    const OWNED: &[&str] = &[
        "c:\\windows",
        "c:\\program files\\windows",
        "c:\\program files (x86)\\windows",
        "c:\\programdata\\microsoft",
        "c:\\programdata\\package cache",
    ];
    let p = path_lower.trim().trim_matches('"').to_lowercase();
    OWNED.iter().any(|root| {
        p == *root || p.starts_with(&format!("{}\\", root))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(name: &str) -> AppSignals {
        AppSignals::from_parts(name, "key", None)
    }

    #[test]
    fn core_temp_never_matches_onecore() {
        let s = sig("Core Temp");
        assert_eq!(match_name(&s, "onecore"), MatchKind::None);
        assert_eq!(match_name(&s, "onecore.stubby"), MatchKind::None);
        assert_eq!(match_name(&s, "core"), MatchKind::None);
        assert_eq!(match_name(&s, "temp"), MatchKind::None);
        // XML containing %TEMP% or "temperature" is not a match either.
        assert_eq!(match_text(&s, "<command>%temp%\\fanctrl.exe</command>"), MatchKind::None);
        assert_eq!(match_text(&s, "cpu temperature monitoring"), MatchKind::None);
    }

    #[test]
    fn core_temp_matches_its_own_names() {
        let s = sig("Core Temp");
        assert_eq!(match_name(&s, "core temp"), MatchKind::Strong);
        assert_eq!(match_name(&s, "coretemp.exe"), MatchKind::Strong);
        assert_eq!(match_name(&s, "core-temp.log"), MatchKind::Strong);
        assert_eq!(match_text(&s, r#"<exec>c:\program files\core temp\coretemp.exe</exec>"#), MatchKind::Strong);
    }

    #[test]
    fn generic_only_app_needs_full_name() {
        // "Temp Cleaner": both tokens generic, so only the full name matches.
        let s = sig("Temp Cleaner");
        assert_eq!(match_name(&s, "temp"), MatchKind::None);
        assert_eq!(match_name(&s, "cleaner"), MatchKind::None);
        assert_eq!(match_name(&s, "temp cleaner"), MatchKind::Strong);
        assert_eq!(match_name(&s, "tempcleaner_setup.exe"), MatchKind::Strong);
    }

    #[test]
    fn distinctive_single_keyword_works_on_names() {
        let s = sig("Firefox");
        assert_eq!(match_name(&s, "firefox"), MatchKind::Strong);
        assert_eq!(match_name(&s, "mozilla firefox"), MatchKind::Strong);
        // compact full-name containment: "firefox" ⊂ "firefoxcache" is a
        // genuine hit ("FirefoxCache" folder belongs to Firefox)
        assert_eq!(match_name(&s, "firefoxcache"), MatchKind::Strong);
        // but a word that doesn't contain the full name never matches
        assert_eq!(match_name(&s, "fire"), MatchKind::None);
        assert_eq!(match_name(&s, "fox"), MatchKind::None);
        let s2 = sig("Epic Games Launcher");
        assert_eq!(match_name(&s2, "launcher"), MatchKind::Weak);
        assert_eq!(match_name(&s2, "epic games"), MatchKind::Weak);
    }

    #[test]
    fn strict_matching_rejects_single_keyword() {
        let s = sig("Epic Games Launcher");
        assert_eq!(match_name_strict(&s, "launcher"), MatchKind::None);
        assert_eq!(match_name_strict(&s, "epic games launcher"), MatchKind::Strong);
    }

    #[test]
    fn compact_never_matches_inside_longer_word() {
        // compact "coretemp" must not be found in "xcoretempy" style junk,
        // but the containment direction we allow is candidate-contains-name,
        // which is what "coretemp.exe" needs.
        let s = sig("Core Temp");
        assert_eq!(match_name(&s, "mycoretemppro.exe"), MatchKind::Strong);
        // ...but a word merely containing one token never matches
        assert_eq!(match_name(&s, "temperature.exe"), MatchKind::None);
    }

    #[test]
    fn windows_owned_paths_blocked() {
        assert!(is_windows_owned_path("C:\\Windows\\System32\\OneCore"));
        assert!(is_windows_owned_path("c:\\windows\\tasks\\microsoft\\onecore"));
        assert!(is_windows_owned_path("C:\\ProgramData\\Microsoft\\Windows Defender"));
        assert!(is_windows_owned_path(r#""C:\Windows\System32\svchost.exe""#));
        assert!(!is_windows_owned_path("C:\\Program Files\\Core Temp\\coretemp.exe"));
        assert!(!is_windows_owned_path("C:\\Users\\bob\\AppData\\Local\\CoreTemp"));
        assert!(!is_windows_owned_path("C:\\Program Files\\WindowsApps\\Nope"));
    }

    #[test]
    fn publisher_tokens_match_whole_words_only() {
        let s = AppSignals::from_parts("Something", "key", Some("ALCPU"));
        assert_eq!(match_name(&s, "alcpu"), MatchKind::Weak);
        assert_eq!(match_name(&s, "alcpi"), MatchKind::None);
    }
}
