use crate::models::AiResponse;

// ponytail: every provider speaks OpenAI-compatible chat, so new providers are
// two lines in resolve_provider. No new HTTP code paths.
fn resolve_provider(provider: &str, endpoint: &str, model: &str) -> Result<(String, String), String> {
    match provider {
        "groq" => Ok((
            "https://api.groq.com/openai/v1/chat/completions".to_string(),
            "openai/gpt-oss-120b".to_string(),
        )),
        "openrouter" => Ok((
            "https://openrouter.ai/api/v1/chat/completions".to_string(),
            "meta-llama/llama-3.1-8b-instruct:free".to_string(),
        )),
        "openai" => Ok((
            "https://api.openai.com/v1/chat/completions".to_string(),
            "gpt-4o-mini".to_string(),
        )),
        "deepseek" => Ok((
            "https://api.deepseek.com/chat/completions".to_string(),
            "deepseek-chat".to_string(),
        )),
        "ollama" => Ok((
            "http://localhost:11434/v1/chat/completions".to_string(),
            default_model(model, "llama3.1:8b"),
        )),
        "custom" => {
            let url = endpoint.trim();
            if !(url.starts_with("https://") || url.starts_with("http://localhost") || url.starts_with("http://127.0.0.1")) {
                return Err("Custom endpoint must use https:// (http:// allowed only for localhost Ollama-style endpoints)".to_string());
            }
            // ponytail: http allowed only for localhost; ceiling is cleartext key leak on LAN, upgrade is https-only plus per-host allowlist.
            if model.trim().is_empty() {
                return Err("Custom model name required".to_string());
            }
            Ok((url.to_string(), model.trim().to_string()))
        }
        _ => Err(format!("Unsupported provider: {}", provider)),
    }
}

fn default_model(model: &str, fallback: &str) -> String {
    if model.trim().is_empty() {
        fallback.to_string()
    } else {
        model.trim().to_string()
    }
}

/// Ollama runs locally and custom endpoints may be keyless (LM Studio etc).
/// Cloud providers always need a key.
fn key_required(provider: &str) -> bool {
    !matches!(provider, "ollama" | "custom")
}

async fn chat_content(
    url: &str,
    model: &str,
    api_key: &str,
    system: &str,
    user: &str,
    max_tokens: u16,
) -> Result<String, String> {
    // ponytail: single 15s timeout beats retry queues; ceiling is slow-provider false failure, upgrade is per-provider timeouts.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Request failed: {}", e))?;
    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "max_tokens": max_tokens,
            "temperature": 0.2
        }));
    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }
    let response = req
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

    body["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            let raw = serde_json::to_string(&body).unwrap_or_default();
            let preview = if raw.len() > 400 { &raw[..400] } else { &raw };
            format!("AI returned no content. Raw: {}", preview)
        })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn ask_ai(
    path: String,
    name: String,
    item_type: String,
    associated_app: String,
    api_key: String,
    provider: String,
    endpoint: String,
    model: String,
) -> Result<AiResponse, String> {
    // ponytail: backend key fallback beats frontend plaintext; ceiling is per-provider Credential Manager entry, upgrade is session-scoped memory cache.
    let resolved_key = if api_key.is_empty() {
        crate::commands::secure_storage::load_api_key_internal(&provider).unwrap_or_default()
    } else {
        api_key
    };
    if resolved_key.is_empty() && key_required(&provider) {
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

    let (url, resolved_model) = resolve_provider(&provider, &endpoint, &model)?;

    let raw_content = chat_content(
        &url,
        &resolved_model,
        &resolved_key,
        "You are a Windows system analyst. Reply with JSON only. No markdown, no preamble. Confidence must be High, Medium, or Low. Recommendation must be delete, keep, or manual review.",
        &prompt,
        260,
    )
    .await?;

    // Try strict JSON first; fall back to forgiving parse of markdown-ish text
    let (assessment, confidence, recommendation) = parse_ai_content(&raw_content);

    Ok(AiResponse {
        assessment,
        confidence,
        recommendation,
    })
}

#[tauri::command]
pub async fn test_ai_connection(
    provider: String,
    endpoint: String,
    model: String,
    api_key: String,
) -> Result<String, String> {
    let resolved_key = if api_key.is_empty() {
        crate::commands::secure_storage::load_api_key_internal(&provider).unwrap_or_default()
    } else {
        api_key
    };
    if resolved_key.is_empty() && key_required(&provider) {
        return Err("API key required".to_string());
    }
    let (url, resolved_model) = resolve_provider(&provider, &endpoint, &model)?;
    chat_content(&url, &resolved_model, &resolved_key, "Reply with {} only.", "ping", 10).await?;
    Ok(format!("Connected — {} answered.", resolved_model))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_resolve() {
        let (url, model) = resolve_provider("groq", "", "").unwrap();
        assert!(url.contains("groq"));
        assert!(!model.is_empty());
        let (url, model) = resolve_provider("ollama", "", "").unwrap();
        assert!(url.contains("11434"));
        assert_eq!(model, "llama3.1:8b");
        let (_, model) = resolve_provider("ollama", "", "qwen2.5:7b").unwrap();
        assert_eq!(model, "qwen2.5:7b");
        assert!(resolve_provider("custom", "not-a-url", "m").is_err());
        assert!(resolve_provider("custom", "https://h/v1/chat/completions", "").is_err());
        assert!(resolve_provider("custom", "http://192.168.1.10/v1", "m").is_err());
        assert!(resolve_provider("custom", "http://localhost:11434/v1/chat/completions", "m").is_ok());
        assert!(resolve_provider("nope", "", "").is_err());
        assert!(key_required("openai"));
        assert!(!key_required("ollama"));
        assert!(!key_required("custom"));
    }
}
