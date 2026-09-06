use crate::models::AiResponse;

#[tauri::command]
pub async fn ask_ai(
    path: String,
    name: String,
    item_type: String,
    associated_app: String,
    api_key: String,
    provider: String,
) -> Result<AiResponse, String> {
    if api_key.is_empty() {
        return Err("API key required".to_string());
    }

    let prompt = format!(
        "Analyze this Windows leftover item:\n\
         Path: {}\n\
         Name: {}\n\
         Type: {}\n\
         Associated App: {}\n\n\
         Reply as compact JSON only (no markdown, no extra keys):\n\
         {{\"assessment\":\"1-2 sentence plain text, no markdown\",\"confidence\":\"High|Medium|Low\",\"recommendation\":\"delete|keep|manual review\"}}\n\
         Rules: assessment must be safe to show directly; keep it under 280 chars.",
        path, name, item_type, associated_app
    );

    let url = match provider.as_str() {
        "groq" => "https://api.groq.com/openai/v1/chat/completions",
        "openrouter" => "https://openrouter.ai/api/v1/chat/completions",
        _ => return Err(format!("Unsupported provider: {}", provider)),
    };

    let model = match provider.as_str() {
        "groq" => "openai/gpt-oss-120b",
        "openrouter" => "meta-llama/llama-3.1-8b-instruct:free",
        _ => "openai/gpt-oss-120b",
    };

    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": "You are a Windows system analyst. Reply with JSON only. No markdown, no preamble. Confidence must be High, Medium, or Low. Recommendation must be delete, keep, or manual review."},
                {"role": "user", "content": prompt}
            ],
            "max_tokens": 260,
            "temperature": 0.2
        }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let err_body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);
        let msg = err_body["error"]["message"]
            .as_str()
            .or_else(|| err_body["message"].as_str())
            .unwrap_or("Unknown provider error");
        return Err(format!("AI provider error ({}): {}", status.as_u16(), msg));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let raw_content = body["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            let raw = serde_json::to_string(&body).unwrap_or_default();
            let preview = if raw.len() > 400 { &raw[..400] } else { &raw };
            format!("AI returned no content. Raw: {}", preview)
        })?;

    // Try strict JSON first; fall back to forgiving parse of markdown-ish text
    let (assessment, confidence, recommendation) = parse_ai_content(&raw_content);

    Ok(AiResponse {
        assessment,
        confidence,
        recommendation,
    })
}

fn strip_md(s: &str) -> String {
    s.replace("**", "")
        .replace("__", "")
        .replace("##", "")
        .trim()
        .to_string()
}

fn normalize_confidence(s: &str) -> String {
    let lower = s.to_lowercase();
    if lower.contains("high") { "High".to_string() }
    else if lower.contains("low") { "Low".to_string() }
    else { "Medium".to_string() }
}

fn normalize_recommendation(s: &str) -> String {
    let lower = s.to_lowercase();
    if lower.contains("delete") { "delete".to_string() }
    else if lower.contains("keep") { "keep".to_string() }
    else { "manual review".to_string() }
}

fn parse_ai_content(raw: &str) -> (String, String, String) {
    // 1) Try JSON - handle ```json fences and surrounding text
    let mut json_candidate = raw.trim();
    if let Some(start) = json_candidate.find('{') {
        if let Some(end) = json_candidate.rfind('}') {
            json_candidate = &json_candidate[start..=end];
        }
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_candidate) {
        let assessment = v.get("assessment").and_then(|x| x.as_str()).unwrap_or("").trim();
        let confidence = v.get("confidence").and_then(|x| x.as_str()).unwrap_or("Medium");
        let recommendation = v.get("recommendation").and_then(|x| x.as_str()).unwrap_or("manual review");
        if !assessment.is_empty() {
            return (
                strip_md(assessment),
                normalize_confidence(confidence),
                normalize_recommendation(recommendation),
            );
        }
    }

    // 2) Fallback: extract from markdown/text like "**Assessment:** ... **Confidence level:** High ..."
    let cleaned = strip_md(raw);
    let lower = cleaned.to_lowercase();

    // Try to slice assessment between "assessment" and "confidence"
    let mut assessment = cleaned.clone();
    if let Some(a_pos) = lower.find("assessment") {
        let after_a = &cleaned[a_pos..];
        // find colon after assessment
        if let Some(colon) = after_a.find(':') {
            let from = &after_a[colon + 1..];
            // cut before confidence/recommendation
            let mut end = from.len();
            for key in ["confidence", "recommendation"] {
                if let Some(p) = from.to_lowercase().find(key) {
                    if p < end { end = p; }
                }
            }
            assessment = from[..end].trim().trim_matches(|c| c == '-' || c == ':' || c == '\n').to_string();
            // if still very long, keep first 2 sentences
            if assessment.len() > 320 {
                assessment = assessment.chars().take(320).collect::<String>() + "...";
            }
        }
    } else {
        // No labels found, just strip and truncate
        assessment = cleaned.chars().take(320).collect::<String>();
    }
    if assessment.is_empty() { assessment = strip_md(raw).chars().take(320).collect(); }

    // Confidence heuristic from full text
    let confidence = if lower.contains("confidence") {
        // look at 40 chars after "confidence"
        if let Some(p) = lower.find("confidence") {
            normalize_confidence(&cleaned[p..std::cmp::min(p+40, cleaned.len())])
        } else { normalize_confidence(&cleaned) }
    } else { normalize_confidence(&cleaned) };

    let recommendation = if lower.contains("recommendation") {
        if let Some(p) = lower.find("recommendation") {
            normalize_recommendation(&cleaned[p..std::cmp::min(p+80, cleaned.len())])
        } else { normalize_recommendation(&cleaned) }
    } else { normalize_recommendation(&cleaned) };

    (assessment.trim().to_string(), confidence, recommendation)
}
