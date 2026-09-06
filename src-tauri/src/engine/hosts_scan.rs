use crate::engine::matching::{match_name, match_text, AppSignals, MatchKind};
use crate::models::{AppInfo, ConfidenceTier, LeftoverItem, LeftoverType};
use std::fs;

/// Read-only hosts file scanner — detects potential leftover blocks for cracked software.
/// Never writes; only reports matching lines for manual review.
pub fn scan_hosts(app: &AppInfo) -> Result<Vec<LeftoverItem>, String> {
    let hosts_path = std::path::Path::new(r"C:\Windows\System32\drivers\etc\hosts");

    // If file missing or unreadable (no admin), return empty gracefully — not an error
    let content = match fs::read_to_string(hosts_path) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };

    let sig = AppSignals::from_app(app);
    // Publisher signals for vendor-wide crack blocks (e.g. adobe, autodesk)
    let pub_sig = if sig.publisher_lower.is_empty() {
        None
    } else {
        Some(AppSignals::from_parts(
            &app.publisher.clone().unwrap_or_default(),
            "",
            None,
        ))
    };

    let mut items = Vec::new();

    for (idx, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Ignore localhost defaults
        let lower = line.to_lowercase();
        // Token/compact matching only — "core" inside "onecore" can never
        // flag an app called "Core Temp" (see engine::matching).
        let mut confidence = 0u32;

        match match_text(&sig, &lower) {
            MatchKind::Strong => confidence = 75,
            MatchKind::Weak => confidence = 55,
            MatchKind::None => {
                // Vendor block (e.g. "0.0.0.0 adobe-dns.adobe.com")
                if let Some(ref ps) = pub_sig {
                    if let MatchKind::Strong | MatchKind::Weak = match_name(ps, &lower) {
                        confidence = 50;
                    }
                }
            }
        }

        // Extra signal: 127.0.0.1 / 0.0.0.0 sinkhole — typical crack block
        let is_sinkhole = lower.starts_with("127.0.0.1") || lower.starts_with("0.0.0.0");

        if confidence > 0 {
            // Bump confidence slightly if sinkhole pattern
            if is_sinkhole && confidence < 80 {
                confidence = (confidence + 10).min(85);
            }
            let tier = if confidence >= 70 {
                ConfidenceTier::High
            } else if confidence >= 45 {
                ConfidenceTier::Medium
            } else {
                ConfidenceTier::Low
            };
            // Path format: hosts:lineNumber:originalLine — keeps System32 untouched, just reports
            let full_path = format!("hosts:{}:{}", idx + 1, raw_line.trim());
            items.push(LeftoverItem {
                id: crate::models::make_leftover_id(&LeftoverType::Hosts, &full_path),
                path: full_path,
                item_type: LeftoverType::Hosts,
                confidence,
                confidence_tier: tier,
                associated_app: app.name.clone(),
                size: None,
            });
        }
    }

    Ok(items)
}
