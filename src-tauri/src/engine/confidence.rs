use crate::models::{AppInfo, ConfidenceTier, LeftoverItem};

const PROTECTED_PATHS: &[&str] = &[
    "c:\\windows",
    "c:\\windows\\system32",
    "c:\\windows\\syswow64",
    "c:\\program files\\windows",
    "c:\\program files (x86)\\windows",
];

const STOPWORDS: &[&str] = &["data", "cache", "settings", "config", "temp", "tmp"];

/// Split a path into lowercase tokens for boundary-safe matching.
fn path_tokens(path_lower: &str) -> Vec<&str> {
    path_lower
        .split(['\\', '/', '.', '_', '-', ' ', ':'])
        .filter(|t| !t.is_empty())
        .collect()
}

pub fn calculate_confidence(item: &LeftoverItem, app: &AppInfo) -> u32 {
    let mut score: i32 = 0;
    let path_lower = item.path.to_lowercase();
    let name_lower = app.name.to_lowercase();

    if let Some(ref install_loc) = app.install_location {
        let loc = install_loc.to_lowercase();
        // Boundary-safe: `C:\...\App` must not match `C:\...\AppX`.
        if path_lower == loc || path_lower.starts_with(&format!("{}\\", loc)) {
            score += 30;
        }
    }

    // Boundary-safe name hit: whole token equals the name (or its compact
    // form), or a full path component equals the name. Bare `contains`
    // boosted unrelated paths ("GIMP" inside "gimpy", "App" inside "AppX").
    let name_compact: String = name_lower.chars().filter(|c| c.is_alphanumeric()).collect();
    let tokens = path_tokens(&path_lower);
    let sep_name = format!("\\{}", name_lower);
    let name_hit = !name_lower.is_empty()
        && (tokens.iter().any(|t| *t == name_lower || *t == name_compact)
            || path_lower.contains(&format!("{sep_name}\\")) 
            || path_lower.ends_with(&sep_name));
    if name_hit {
        score += 40;
    } else if let Some(ref publisher) = app.publisher {
        if path_lower.contains(&publisher.to_lowercase()) {
            score += 20;
        }
    }

    let fuzzy = strsim::jaro_winkler(&path_lower, &name_lower);
    if fuzzy > 0.85 {
        score += 20;
    }

    // Hosts/Task are read-only reports under System32 by design — don't penalize
    let is_hosts_or_task = matches!(
        item.item_type,
        crate::models::LeftoverType::Hosts | crate::models::LeftoverType::Task
    );
    if !is_hosts_or_task {
        for protected in PROTECTED_PATHS {
            if path_lower.starts_with(protected) {
                score -= 50;
                break;
            }
        }
    }

    // Token-boundary stopwords: `\cache` must not match `\database`.
    if path_tokens(&path_lower).iter().any(|t| STOPWORDS.contains(t)) {
        score -= 30;
    }

    score.max(0) as u32
}

pub fn tier_from_score(score: u32) -> ConfidenceTier {
    match score {
        70.. => ConfidenceTier::High,
        40..=69 => ConfidenceTier::Medium,
        _ => ConfidenceTier::Low,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AppInfo, LeftoverItem, LeftoverType};

    fn app(name: &str, publisher: Option<&str>, install_location: Option<&str>) -> AppInfo {
        AppInfo {
            id: "key".into(),
            name: name.into(),
            version: None,
            publisher: publisher.map(|s| s.to_string()),
            install_location: install_location.map(|s| s.to_string()),
            estimated_size: None,
            uninstall_string: None,
            install_date: None,
            icon: None,
        }
    }

    fn item(item_type: LeftoverType, path: &str) -> LeftoverItem {
        LeftoverItem {
            id: crate::models::make_leftover_id(&item_type, path),
            path: path.into(),
            item_type,
            confidence: 0,
            confidence_tier: ConfidenceTier::Low,
            associated_app: "App".into(),
            size: None,
        }
    }

    #[test]
    fn tier_boundaries() {
        assert!(matches!(tier_from_score(70), ConfidenceTier::High));
        assert!(matches!(tier_from_score(100), ConfidenceTier::High));
        assert!(matches!(tier_from_score(69), ConfidenceTier::Medium));
        assert!(matches!(tier_from_score(40), ConfidenceTier::Medium));
        assert!(matches!(tier_from_score(39), ConfidenceTier::Low));
        assert!(matches!(tier_from_score(0), ConfidenceTier::Low));
    }

    #[test]
    fn exact_name_in_path_is_high() {
        let a = app("AcmeApp", None, None);
        let i = item(LeftoverType::Folder, r"C:\Users\u\AppData\Roaming\AcmeApp");
        assert!(calculate_confidence(&i, &a) >= 40);
    }

    #[test]
    fn install_location_prefix_boost() {
        let a = app("AcmeApp", None, Some(r"C:\Program Files\AcmeApp"));
        let i = item(LeftoverType::Folder, r"C:\Program Files\AcmeApp\data");
        assert!(calculate_confidence(&i, &a) >= 30);
    }

    #[test]
    fn publisher_match_boost() {
        let a = app("SomeEditor", Some("Acme Corp"), None);
        let i = item(LeftoverType::Registry, r"HKEY_CURRENT_USER\SOFTWARE\Acme Corp");
        assert!(calculate_confidence(&i, &a) >= 20);
    }

    #[test]
    fn protected_windows_path_penalized() {
        let a = app("AcmeApp", None, None);
        // Even with the app name present, C:\Windows paths must be penalized hard.
        let i = item(LeftoverType::File, r"C:\Windows\System32\drivers\AcmeApp.sys");
        let score = calculate_confidence(&i, &a);
        assert!(score <= 10, "score was {}", score);
    }

    #[test]
    fn hosts_and_tasks_not_penalized_for_system_location() {
        let a = app("CrackedGame", None, None);
        let h = item(LeftoverType::Hosts, "hosts:12:127.0.0.1 crackedgame.example");
        let t = item(LeftoverType::Task, r"C:\Windows\System32\Tasks\CrackedGame");
        // Score is allowed to be positive for read-only report types under System32.
        assert!(calculate_confidence(&h, &a) >= 40);
        assert!(calculate_confidence(&t, &a) >= 40);
    }

    #[test]
    fn generic_stopword_reduces_score() {
        let a = app("AcmeApp", None, None);
        let i = item(LeftoverType::Folder, r"C:\Users\u\AppData\Local\AcmeApp\cache");
        let without_stop = calculate_confidence(&item(LeftoverType::Folder, r"C:\Users\u\AppData\Local\AcmeApp\logs"), &a);
        let with_stop = calculate_confidence(&i, &a);
        assert!(with_stop < without_stop);
    }

    #[test]
    fn unrelated_path_scores_zero() {
        let a = app("TotallyUnrelatedApp", None, None);
        let i = item(LeftoverType::File, r"C:\Users\u\AppData\Roaming\Chrome\Default\Cache");
        assert_eq!(calculate_confidence(&i, &a), 0);
    }

    #[test]
    fn substring_name_does_not_boost() {
        // "GIMP" inside "gimpy" is a collision, not a match.
        let a = app("GIMP", None, None);
        let i = item(LeftoverType::Folder, r"C:\Users\u\AppData\Roaming\gimpy");
        assert_eq!(calculate_confidence(&i, &a), 0);
    }

    #[test]
    fn install_location_needs_separator_boundary() {
        // `C:\...\App` must not boost `C:\...\AppX`.
        let a = app("App", None, Some(r"C:\Program Files\App"));
        let i = item(LeftoverType::Folder, r"C:\Program Files\AppX\data");
        assert!(calculate_confidence(&i, &a) < 30);
    }

    #[test]
    fn stopword_needs_token_boundary() {
        // `\database` is not the `\data` stopword.
        let a = app("AcmeApp", None, None);
        let with_db = calculate_confidence(&item(LeftoverType::Folder, r"C:\Users\u\AppData\Local\AcmeApp\database"), &a);
        let with_data = calculate_confidence(&item(LeftoverType::Folder, r"C:\Users\u\AppData\Local\AcmeApp\data"), &a);
        assert!(with_db > with_data);
    }

    #[test]
    fn ids_are_deterministic() {
        let a = item(LeftoverType::File, r"C:\Path\To\File.txt");
        let b = item(LeftoverType::File, r"C:\Path\To\File.txt");
        let c = item(LeftoverType::Folder, r"C:\Path\To\File.txt");
        assert_eq!(a.id, b.id);
        assert_ne!(a.id, c.id); // type participates in the id
    }
}
