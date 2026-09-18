use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

/// Which HTTP shape a transcription server speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttApi {
    /// `/audio/transcriptions` with a `model` field: OpenAI, Groq, Speaches, LocalAI.
    OpenAi,
    /// whisper.cpp's own server: `/inference`, no `model` field, one model per process.
    WhisperCpp,
}

pub fn api_for(value: Option<&str>) -> SttApi {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("whispercpp") | Some("whisper.cpp") | Some("inference") => SttApi::WhisperCpp,
        _ => SttApi::OpenAi,
    }
}

/// Sends a WAV to a transcription server: OpenAI-compatible by default, whisper.cpp's
/// `/inference` when the profile says so. Both answer with `{"text": ...}`.
pub async fn transcribe(client: &reqwest::Client, cfg: &Config, wav: Vec<u8>) -> Result<String> {
    let key = cfg
        .stt_key()
        .context("no speech-to-text API key: set stt.api_key in config.json or STFU_STT_API_KEY")?;
    let base = cfg.stt().base_url.trim_end_matches('/');
    let api = api_for(cfg.stt().api.as_deref());
    let url = match api {
        SttApi::OpenAi => format!("{base}/audio/transcriptions"),
        SttApi::WhisperCpp => format!("{base}/inference"),
    };

    let file = reqwest::multipart::Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;
    let mut form = reqwest::multipart::Form::new()
        .part("file", file)
        .text("response_format", "json");
    // whisper.cpp serves one model per process and rejects nothing, but sending a model it does
    // not know is pointless; the OpenAI shape requires it.
    if api == SttApi::OpenAi {
        form = form.text("model", cfg.stt().model.clone());
    }
    if !cfg.stt().language.trim().is_empty() {
        form = form.text("language", cfg.stt().language.trim().to_string());
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whispercpp_is_recognised_by_its_aliases() {
        assert_eq!(api_for(Some("whispercpp")), SttApi::WhisperCpp);
        assert_eq!(api_for(Some("whisper.cpp")), SttApi::WhisperCpp);
        assert_eq!(api_for(Some(" WhisperCpp ")), SttApi::WhisperCpp);
    }

    #[test]
    fn everything_else_is_the_openai_shape() {
        assert_eq!(api_for(None), SttApi::OpenAi);
        assert_eq!(api_for(Some("")), SttApi::OpenAi);
        assert_eq!(api_for(Some("openai")), SttApi::OpenAi);
    }
}
