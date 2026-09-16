//! Tauri commands used by the settings window.

use crate::config::Config;
use crate::{audio, llm, stt, SharedConfig};
use tauri::State;

#[tauri::command]
pub fn get_config(state: State<'_, SharedConfig>) -> Config {
    state.read().unwrap().clone()
}

#[tauri::command]
pub fn config_path() -> String {
    Config::path().map(|p| p.display().to_string()).unwrap_or_default()
}

#[tauri::command]
pub fn save_config(state: State<'_, SharedConfig>, cfg: Config) -> Result<(), String> {
    cfg.save().map_err(|e| format!("{e:#}"))?;
    *state.write().unwrap() = cfg;
    log::info!("config saved");
    Ok(())
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
