use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Keys that must all be held to record. Names follow rdev's `Key` enum:
    /// Function, ControlLeft, ControlRight, MetaLeft, MetaRight, Alt, AltGr, ShiftLeft, ShiftRight.
    pub hotkey: Vec<String>,
    pub stt: SttConfig,
    pub llm: LlmConfig,
    /// Start with the OS session so dictation is always available. Toggle from the tray menu.
    #[serde(default = "default_true")]
    pub launch_at_login: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttConfig {
    /// OpenAI-compatible base URL exposing `/audio/transcriptions`.
    pub base_url: String,
    pub model: String,
    /// Leave empty to read from the STFU_STT_API_KEY environment variable.
    #[serde(default)]
    pub api_key: String,
    /// ISO-639-1 code (e.g. "en"); empty = let the model detect.
    #[serde(default)]
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Set to false to paste the raw transcript without any cleanup.
    pub enabled: bool,
    /// OpenAI-compatible base URL exposing `/chat/completions`.
    pub base_url: String,
    pub model: String,
    /// Leave empty to read from the STFU_LLM_API_KEY environment variable.
    #[serde(default)]
    pub api_key: String,
    /// Seconds to wait for cleanup before falling back to the raw transcript.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    8
}

impl Default for Config {
    fn default() -> Self {
        let hotkey = if cfg!(target_os = "macos") {
            vec!["Function".to_string()]
        } else {
            vec!["ControlLeft".to_string(), "MetaLeft".to_string()]
        };
        Self {
            hotkey,
            stt: SttConfig {
                base_url: "https://api.groq.com/openai/v1".into(),
                model: "whisper-large-v3-turbo".into(),
                api_key: String::new(),
                language: String::new(),
            },
            llm: LlmConfig {
                enabled: true,
                // Groq's free tier covers both roles with one key, so it is the default for both.
                base_url: "https://api.groq.com/openai/v1".into(),
                model: "qwen/qwen3.8-27b".into(),
                api_key: String::new(),
                timeout_secs: default_timeout(),
            },
            launch_at_login: true,
        }
    }
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("no config directory on this platform")?
            .join("stfu");
        Ok(dir.join("config.json"))
    }

    /// Loads the config, writing a default file on first run so the user has something to edit.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            let cfg = Self::default();
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(&path, serde_json::to_string_pretty(&cfg)?)?;
            log::info!("wrote default config to {}", path.display());
            return Ok(cfg);
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg: Self =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// True when a dictation could not succeed with the current keys.
    pub fn needs_setup(&self) -> bool {
        self.stt_key().is_none() || (self.llm.enabled && self.llm_key().is_none())
    }

    pub fn stt_key(&self) -> Option<String> {
        first_non_empty(&self.stt.api_key, &["STFU_STT_API_KEY", "GROQ_API_KEY", "OPENAI_API_KEY"])
    }

    pub fn llm_key(&self) -> Option<String> {
        first_non_empty(&self.llm.api_key, &["STFU_LLM_API_KEY", "OPENCODE_API_KEY"])
    }
}

fn first_non_empty(explicit: &str, env_names: &[&str]) -> Option<String> {
    if !explicit.trim().is_empty() {
        return Some(explicit.trim().to_string());
    }
    env_names
        .iter()
        .filter_map(|n| std::env::var(n).ok())
        .find(|v| !v.trim().is_empty())
}
