pub fn is_excluded_path(path: &str, excluded: &[String]) -> bool {
    let lower = path.to_lowercase();
    for e in excluded {
        let el = e.to_lowercase();
        if lower == el || lower.starts_with(&format!("{}\\", el)) || el == lower {
            return true;
        }
    }
    false
}

pub fn is_excluded_host(line: &str, excluded: &[String]) -> bool {
    let trimmed = line.trim().to_lowercase();
    for e in excluded {
        if trimmed == e.trim().to_lowercase() {
            return true;
        }
    }
    false
}
