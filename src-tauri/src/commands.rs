//! Tauri commands used by the settings window.

use crate::config::Config;
use crate::{audio, lang, llm, stt, SharedConfig};
use tauri::menu::{CheckMenuItem, Submenu};
use tauri::{State, Wry};

/// The tray's language submenu, so a change made in Settings updates the menu too.
pub struct LangMenu {
    pub items: Vec<CheckMenuItem<Wry>>,
    pub submenu: Submenu<Wry>,
}

#[tauri::command]
pub fn get_config(state: State<'_, SharedConfig>) -> Config {
    state.read().unwrap().clone()
}

#[tauri::command]
pub fn config_path() -> String {
    Config::path().map(|p| p.display().to_string()).unwrap_or_default()
}

#[tauri::command]
pub fn save_config(
    state: State<'_, SharedConfig>,
    lang_menu: State<'_, LangMenu>,
    cfg: Config,
) -> Result<(), String> {
    cfg.save().map_err(|e| format!("{e:#}"))?;
    let code = cfg.stt.language.trim().to_string();
    for (item, (c, _)) in lang_menu.items.iter().zip(lang::LANGUAGES) {
        let _ = item.set_checked(*c == code);
    }
    let _ = lang_menu.submenu.set_text(format!("Language: {}", lang::label(&code)));
    *state.write().unwrap() = cfg;
    log::info!("config saved");
    Ok(())
}

#[derive(serde::Serialize)]
pub struct Language {
    pub code: String,
    pub label: String,
}

/// The language list shown in Settings; same source as the tray submenu.
#[tauri::command]
pub fn languages() -> Vec<Language> {
    lang::LANGUAGES
        .iter()
        .map(|(code, label)| Language { code: (*code).into(), label: (*label).into() })
        .collect()
}

/// Sends half a second of silence to the transcription endpoint. A 200 proves URL, model and key.
#[tauri::command]
pub async fn test_stt(cfg: Config) -> Result<String, String> {
    let client = reqwest::Client::new();
    let wav = audio::silent_wav(0.6).map_err(|e| format!("{e:#}"))?;
    let t = std::time::Instant::now();
    stt::transcribe(&client, &cfg, wav)
        .await
        .map(|_| format!("Works. {} responded in {} ms.", cfg.stt.model, t.elapsed().as_millis()))
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn test_llm(cfg: Config) -> Result<String, String> {
    let client = reqwest::Client::new();
    let t = std::time::Instant::now();
    llm::cleanup(&client, &cfg, "um so this is like a test, a test of the, uh, cleanup model")
        .await
        .map(|out| format!("Works in {} ms: “{}”", t.elapsed().as_millis(), out.chars().take(80).collect::<String>()))
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("refusing to open a non-http URL".into());
    }
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(&url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd").args(["/C", "start", "", &url]).spawn();
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(&url).spawn();
    result.map(|_| ()).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub free: bool,
    pub wire: String,
}

/// Live model list from OpenCode Zen. Free models are the ones Zen prices at zero, which today
/// all carry a "-free" suffix or are the two long-standing free ids.
#[tauri::command]
pub async fn zen_models(api_key: String) -> Result<Vec<ModelInfo>, String> {
    let base = "https://opencode.ai/zen/v1";
    let client = reqwest::Client::new();
    let mut req = client.get(format!("{base}/models"));
    if !api_key.trim().is_empty() {
        req = req.bearer_auth(api_key.trim());
    }
    let v: serde_json::Value = req
        .send()
        .await
        .map_err(|e| format!("could not reach OpenCode Zen: {e}"))?
        .json()
        .await
        .map_err(|e| format!("unexpected response from OpenCode Zen: {e}"))?;
    let mut out: Vec<ModelInfo> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str().map(str::to_string))
        .map(|id| {
            let free = id.ends_with("-free") || id == "big-pickle" || id == "union-alpha";
            let wire = format!("{:?}", llm::wire_for(base, &id, None)).to_ascii_lowercase();
            ModelInfo { id, free, wire }
        })
        .collect();
    out.sort_by(|a, b| b.free.cmp(&a.free).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

/// The OpenCode CLI stores provider credentials in auth.json. If the user has already logged in
/// to Zen there, reuse that key instead of asking them to copy it again.
#[tauri::command]
pub fn import_opencode_key() -> Result<String, String> {
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/share/opencode/auth.json"));
    }
    if let Some(data) = dirs::data_dir() {
        candidates.push(data.join("opencode/auth.json"));
    }
    if let Some(data) = dirs::data_local_dir() {
        candidates.push(data.join("opencode/auth.json"));
    }
    for path in &candidates {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        if let Some(key) = v["opencode"]["key"].as_str() {
            log::info!("imported OpenCode Zen key from {}", path.display());
            return Ok(key.to_string());
        }
    }
    Err("No OpenCode Zen credentials found. Run `opencode auth login`, pick OpenCode Zen, or paste a key from opencode.ai/zen.".into())
}

#[derive(serde::Serialize)]
pub struct Permission {
    /// Stable key used by the UI and by `request_permission`.
    pub key: String,
    pub label: String,
    pub granted: bool,
    /// Why the app needs it, in the user's terms.
    pub detail: String,
    /// True while the OS is still willing to show its own prompt.
    pub can_prompt: bool,
}

/// Every OS permission the app needs, with its current state. Platform-specific: macOS gates
/// microphone, accessibility and input monitoring; Windows only really gates the microphone.
#[tauri::command]
pub fn permission_status() -> Vec<Permission> {
    #[cfg(target_os = "macos")]
    {
        use crate::permissions as p;
        let mic = p::microphone_granted();
        let ax = p::accessibility_trusted();
        let im = p::input_monitoring_granted();
        vec![
            Permission {
                key: "microphone".into(),
                label: "Microphone".into(),
                granted: mic,
                detail: "Records your voice while the hotkey is held. Without it macOS hands the app silence instead of an error.".into(),
                can_prompt: !mic,
            },
            Permission {
                key: "input_monitoring".into(),
                label: "Input Monitoring".into(),
                granted: im,
                detail: "Lets the app notice the hotkey being held anywhere. Without it the hotkey does nothing at all.".into(),
                can_prompt: !im,
            },
            Permission {
                key: "accessibility".into(),
                label: "Accessibility".into(),
                granted: ax,
                detail: "Pastes the finished text into whatever app you are typing in. The app restarts itself once you grant this.".into(),
                can_prompt: !ax,
            },
        ]
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mic = crate::audio::input_device_available();
        vec![Permission {
            key: "microphone".into(),
            label: "Microphone".into(),
            granted: mic,
            detail: "Records your voice while the hotkey is held. If this is red, allow desktop apps to use the microphone in Windows privacy settings.".into(),
            can_prompt: false,
        }]
    }
}

/// Ask the OS to show its own prompt for a permission, or open the relevant settings pane when
/// the OS will no longer prompt (macOS only ever asks once).
#[tauri::command]
pub fn request_permission(key: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use crate::permissions as p;
        match key.as_str() {
            "microphone" => {
                if p::microphone_granted() {
                    return Ok(());
                }
                p::request_microphone(|granted| log::info!("microphone prompt result: {granted}"));
                open_pane("microphone")
            }
            "input_monitoring" => {
                if !p::request_input_monitoring() {
                    open_pane("input_monitoring")?;
                }
                Ok(())
            }
            "accessibility" => {
                p::request_accessibility();
                open_pane("accessibility")
            }
            other => Err(format!("unknown permission: {other}")),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = key;
        open_pane("microphone")
    }
}

/// Opens the OS settings page for a permission. Kept separate from `open_url` so the app never
/// opens an arbitrary non-http scheme on request from the webview.
#[tauri::command]
pub fn open_pane(key: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let url = match key {
        "microphone" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
        "accessibility" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        "input_monitoring" => "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent",
        other => return Err(format!("unknown settings pane: {other}")),
    };
    #[cfg(target_os = "windows")]
    let url = match key {
        "microphone" => "ms-settings:privacy-microphone",
        other => return Err(format!("unknown settings pane: {other}")),
    };
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let url = {
        let _ = key;
        return Ok(());
    };

    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let spawned = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    spawned.map(|_| ()).map_err(|e| e.to_string())
}
