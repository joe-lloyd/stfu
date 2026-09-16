use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

/// Sends a WAV to an OpenAI-compatible `/audio/transcriptions` endpoint (OpenAI, Groq, ...).
pub async fn transcribe(client: &reqwest::Client, cfg: &Config, wav: Vec<u8>) -> Result<String> {
    let key = cfg
        .stt_key()
        .context("no speech-to-text API key: set stt.api_key in config.json or STFU_STT_API_KEY")?;
    let url = format!("{}/audio/transcriptions", cfg.stt.base_url.trim_end_matches('/'));

    let file = reqwest::multipart::Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;
    let mut form = reqwest::multipart::Form::new()
        .part("file", file)
        .text("model", cfg.stt.model.clone())
        .text("response_format", "json");
    if !cfg.stt.language.trim().is_empty() {
        form = form.text("language", cfg.stt.language.trim().to_string());
    }

    let resp = client
        .post(&url)
        .bearer_auth(key)
        .multipart(form)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .context("speech-to-text request failed")?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("speech-to-text HTTP {status}: {}", body.chars().take(300).collect::<String>()));
    }
    let parsed: TranscriptionResponse =
        serde_json::from_str(&body).context("speech-to-text returned unexpected JSON")?;
    Ok(parsed.text.trim().to_string())
}
