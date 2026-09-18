use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A named pair of providers you can switch between: cloud models for everyday use, a fully local
/// set for anything that must not leave the machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub stt: SttConfig,
    pub llm: LlmConfig,
}

impl Profile {
    /// Everything stays on this machine: whisper.cpp for speech, Ollama for clean-up.
    pub fn local() -> Self {
        Self {
            name: "Local (offline)".into(),
            stt: SttConfig {
                base_url: "http://localhost:8080".into(),
                api: Some("whispercpp".into()),
                model: String::new(),
                api_key: "local".into(),
                language: default_language(),
            },
            llm: LlmConfig {
                enabled: true,
                base_url: "http://localhost:11434/v1".into(),
                model: "llama3.2:3b".into(),
                api_key: "ollama".into(),
                timeout_secs: 20,
                wire: None,
            },
        }
    }

    /// The free cloud default: Groq for both roles, one key.
    pub fn cloud() -> Self {
        Self {
            name: "Cloud (free)".into(),
            stt: SttConfig {
                base_url: "https://api.groq.com/openai/v1".into(),
                api: None,
                model: "whisper-large-v3-turbo".into(),
                api_key: String::new(),
                language: default_language(),
            },
            llm: LlmConfig {
                enabled: true,
                base_url: "https://api.groq.com/openai/v1".into(),
                model: "qwen/qwen3.8-27b".into(),
                api_key: String::new(),
                timeout_secs: default_timeout(),
                wire: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Keys that must all be held to record. Names follow rdev's `Key` enum:
    /// Function, ControlLeft, ControlRight, MetaLeft, MetaRight, Alt, AltGr, ShiftLeft, ShiftRight.
    pub hotkey: Vec<String>,
    /// Name of the profile currently in use.
    #[serde(default)]
    pub active_profile: String,
    #[serde(default)]
    pub profiles: Vec<Profile>,

    // Pre-profiles config had stt/llm at the top level. Kept so an existing install keeps working;
    // `migrate` folds them into a profile and they stop being written.
    #[serde(default, skip_serializing)]
    stt: Option<SttConfig>,
    #[serde(default, skip_serializing)]
    llm: Option<LlmConfig>,
    /// Start with the OS session so dictation is always available. Toggle from the tray menu.
    #[serde(default = "default_true")]
    pub launch_at_login: bool,
    /// Check GitHub for a newer release in the background and install it.
    #[serde(default = "default_true")]
    pub auto_update: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttConfig {
    /// Base URL of the transcription server.
    pub base_url: String,
    /// Which shape the server speaks: "openai" (`/audio/transcriptions`, used by OpenAI, Groq,
    /// Speaches, LocalAI) or "whispercpp" (`/inference`, used by whisper.cpp's own server).
    /// Empty or missing means OpenAI.
    #[serde(default)]
    pub api: Option<String>,
    pub model: String,
    /// Leave empty to read from the STFU_STT_API_KEY environment variable.
    #[serde(default)]
    pub api_key: String,
    /// ISO-639-1 code (e.g. "en"); empty = let the model detect. Defaults to English.
    #[serde(default = "default_language")]
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
    /// Force a wire format: "chat", "responses", "messages" or "gemini". Empty = auto (Zen models
    /// are routed by id; everything else is chat completions).
    #[serde(default)]
    pub wire: Option<String>,
}

fn default_timeout() -> u64 {
    8
}

/// English rather than auto-detect: auto-detect misreads short or accented utterances often
/// enough to be annoying, and most people dictate in one language most of the time.
fn default_language() -> String {
    "en".to_string()
}

impl Default for Config {
    fn default() -> Self {
        let hotkey = if cfg!(target_os = "macos") {
            vec!["Function".to_string()]
        } else {
            vec!["ControlLeft".to_string(), "MetaLeft".to_string()]
        };
        let cloud = Profile::cloud();
        Self {
            hotkey,
            active_profile: cloud.name.clone(),
            profiles: vec![cloud, Profile::local()],
            stt: None,
            llm: None,
            launch_at_login: true,
            auto_update: true,
        }
    }
}

impl Config {
    /// The profile in use, falling back to the first one so the app never has nothing to talk to.
    pub fn active(&self) -> &Profile {
        self.profiles
            .iter()
            .find(|p| p.name == self.active_profile)
            .or_else(|| self.profiles.first())
            .expect("config always has at least one profile")
    }

    pub fn active_mut(&mut self) -> &mut Profile {
        let name = self.active_profile.clone();
        let idx = self
            .profiles
            .iter()
            .position(|p| p.name == name)
            .unwrap_or(0);
        &mut self.profiles[idx]
    }

    pub fn stt(&self) -> &SttConfig {
        &self.active().stt
    }

    pub fn llm(&self) -> &LlmConfig {
        &self.active().llm
    }

    /// Folds a pre-profiles config into a profile, and guarantees there is always a local option
    /// to switch to. Safe to call on an already-migrated config.
    fn migrate(&mut self) {
        if self.profiles.is_empty() {
            let mut cloud = Profile::cloud();
            if let (Some(stt), Some(llm)) = (self.stt.take(), self.llm.take()) {
                log::info!("migrating pre-profiles config into a profile");
                cloud.stt = stt;
                cloud.llm = llm;
            }
            self.profiles.push(cloud);
        }
        if !self.profiles.iter().any(|p| p.name == Profile::local().name) {
            self.profiles.push(Profile::local());
        }
        if !self.profiles.iter().any(|p| p.name == self.active_profile) {
            self.active_profile = self.profiles[0].name.clone();
        }
    }

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
        let mut cfg: Self =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let before = serde_json::to_string(&cfg).unwrap_or_default();
        cfg.migrate();
        if serde_json::to_string(&cfg).unwrap_or_default() != before {
            let _ = cfg.save();
        }
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
        self.stt_key().is_none() || (self.llm().enabled && self.llm_key().is_none())
    }

    /// The speech-to-text key, or a placeholder for a local server that does not need one.
    pub fn stt_key(&self) -> Option<String> {
        first_non_empty(&self.stt().api_key, &["STFU_STT_API_KEY", "GROQ_API_KEY", "OPENAI_API_KEY"])
            .or_else(|| is_local_url(&self.stt().base_url).then(|| "local".to_string()))
    }

    pub fn llm_key(&self) -> Option<String> {
        first_non_empty(&self.llm().api_key, &["STFU_LLM_API_KEY", "OPENCODE_API_KEY"])
            .or_else(|| is_local_url(&self.llm().base_url).then(|| "local".to_string()))
    }
}

/// True for a server running on this machine. Those need no API key, so the app must not nag for
/// one or refuse to dictate when a fully local profile is selected.
pub fn is_local_url(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    ["localhost", "127.0.0.1", "0.0.0.0", "[::1]", "::1"]
        .iter()
        .any(|h| u.contains(h))
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_urls_need_no_api_key() {
        assert!(is_local_url("http://localhost:8080"));
        assert!(is_local_url("http://127.0.0.1:11434/v1"));
        assert!(is_local_url("http://[::1]:8080"));
        assert!(!is_local_url("https://api.groq.com/openai/v1"));
    }

    #[test]
    fn a_pre_profiles_config_keeps_its_providers() {
        let old = r#"{
            "hotkey": ["Function"],
            "stt": {"base_url": "https://api.groq.com/openai/v1", "model": "whisper-large-v3-turbo", "api_key": "secret", "language": "nl"},
            "llm": {"enabled": true, "base_url": "https://api.groq.com/openai/v1", "model": "qwen/qwen3.8-27b", "api_key": "secret", "timeout_secs": 8}
        }"#;
        let mut cfg: Config = serde_json::from_str(old).expect("old config parses");
        cfg.migrate();

        assert_eq!(cfg.profiles.len(), 2, "the local profile is added alongside");
        assert_eq!(cfg.active_profile, "Cloud (free)");
        assert_eq!(cfg.stt().api_key, "secret", "the key survives migration");
        assert_eq!(cfg.stt().language, "nl", "so does the language");
        assert_eq!(cfg.llm().model, "qwen/qwen3.8-27b");
        assert!(cfg.profiles.iter().any(|p| p.name == "Local (offline)"));
    }

    #[test]
    fn migrating_twice_changes_nothing() {
        let mut cfg = Config::default();
        let before = serde_json::to_string(&cfg).unwrap();
        cfg.migrate();
        assert_eq!(serde_json::to_string(&cfg).unwrap(), before);
    }

    #[test]
    fn a_local_profile_is_usable_without_any_key() {
        let mut cfg = Config::default();
        cfg.active_profile = "Local (offline)".into();
        assert!(cfg.stt_key().is_some());
        assert!(cfg.llm_key().is_some());
        assert!(!cfg.needs_setup(), "a local profile must not nag for keys");
    }
}
