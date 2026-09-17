//! Silent auto-update from GitHub Releases via tauri-plugin-updater.
//!
//! Releases are signed with the same certificate every time (CI imports it from a secret), so on
//! macOS the Accessibility / Input Monitoring / Microphone grants carry over: to TCC the updated
//! app is the same app. Update payloads are additionally minisign-verified with the pubkey in
//! tauri.conf.json before install.

use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK_AFTER: Duration = Duration::from_secs(20);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_AFTER).await;
        loop {
            // Read the setting each time so turning it off in Settings takes effect immediately.
            let enabled = app
                .try_state::<crate::SharedConfig>()
                .map(|c| c.read().unwrap().auto_update)
                .unwrap_or(true);
            if enabled {
                if let Err(e) = check_and_install(&app).await {
                    log::warn!("update check failed: {e:#}");
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// Triggered from the tray menu.
pub fn check_now(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match check_and_install(&app).await {
            Ok(None) => log::info!("no update available (running {})", env!("CARGO_PKG_VERSION")),
            Ok(Some(_)) => {}
            Err(e) => log::warn!("update check failed: {e:#}"),
        }
    });
}

/// Returns the new version when one was installed. The app restarts, so this does not return
/// normally in that case.
pub async fn check_and_install(app: &AppHandle) -> anyhow::Result<Option<String>> {
    let updater = app.updater()?;
    let Some(update) = updater.check().await? else {
        return Ok(None);
    };
    log::info!(
        "update available: {} -> {}; downloading",
        update.current_version,
        update.version
    );
    update
        .download_and_install(|_chunk, _total| {}, || log::info!("update downloaded, installing"))
        .await?;
    log::info!("update installed; restarting");
    app.restart();
}
