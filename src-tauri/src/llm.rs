use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

const SYSTEM_PROMPT: &str = r#"You clean up raw speech-to-text dictation. Return ONLY the cleaned text, nothing else: no preamble, no quotes, no explanations, no markdown code fences.

Rules:
- Remove filler words (um, uh, like, you know) and false starts.
- Apply self-corrections: "send it Tuesday, no, Wednesday" becomes "send it Wednesday".
- Fix punctuation, capitalisation and obvious transcription errors. Keep the speaker's words and meaning; do not paraphrase, summarise, or add content.
- If the speaker clearly lists several items, format them as a bulleted list using "- " lines.
- Keep the same language as the speaker.
- Preserve technical terms, code identifiers, URLs and commands exactly.
- Spoken formatting commands like "new paragraph" or "new line" become actual breaks."#;

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice {
    message: Message,
}
#[derive(Deserialize)]
struct Message {
    content: Option<String>,
}

/// Cleans a transcript through an OpenAI-compatible `/chat/completions` endpoint.
pub async fn cleanup(client: &reqwest::Client, cfg: &Config, transcript: &str) -> Result<String> {
    let key = cfg
        .llm_key()
        .context("no LLM API key: set llm.api_key in config.json or STFU_LLM_API_KEY")?;
    let url = format!("{}/chat/completions", cfg.llm.base_url.trim_end_matches('/'));
    let body = json!({
        "model": cfg.llm.model,
        "temperature": 0.2,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": transcript}
        ]
    });
    let resp = client
        .post(&url)
        .bearer_auth(key)
        .json(&body)
        .timeout(Duration::from_secs(cfg.llm.timeout_secs.max(1)))
        .send()
        .await
        .context("LLM request failed")?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("LLM HTTP {status}: {}", text.chars().take(300).collect::<String>()));
    }
    let parsed: ChatResponse = serde_json::from_str(&text).context("LLM returned unexpected JSON")?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .unwrap_or_default();
    let cleaned = strip_fences(content.trim());
    if cleaned.is_empty() {
        return Err(anyhow!("LLM returned empty text"));
    }
    Ok(cleaned)
}

/// Some models wrap output in ``` fences despite instructions; unwrap if the whole reply is one block.
fn strip_fences(s: &str) -> String {
    if s.starts_with("```") && s.ends_with("```") {
        let inner = s.trim_start_matches("```");
        let inner = inner.split_once('\n').map(|(_, rest)| rest).unwrap_or(inner);
        return inner.trim_end_matches("```").trim().to_string();
    }
    s.to_string()
}
