//! Silent auto-update from GitHub Releases via tauri-plugin-updater.
//!
//! Releases are signed with the same certificate every time (CI imports it from a secret), so on
//! macOS the Accessibility / Input Monitoring / Microphone grants carry over: to TCC the updated
//! app is the same app. Update payloads are additionally minisign-verified with the pubkey in
//! tauri.conf.json before install.

use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK_AFTER: Duration = Duration::from_secs(20);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_AFTER).await;
        loop {
            if let Err(e) = check_and_install(&app).await {
                log::warn!("update check failed: {e:#}");
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// Triggered from the tray menu.
pub fn check_now(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match check_and_install(&app).await {
            Ok(false) => log::info!("no update available (running {})", env!("CARGO_PKG_VERSION")),
            Ok(true) => {}
            Err(e) => log::warn!("update check failed: {e:#}"),
        }
    });
}

/// Returns Ok(true) if an update was installed (the app restarts and does not return).
async fn check_and_install(app: &AppHandle) -> anyhow::Result<bool> {
    let updater = app.updater()?;
    let Some(update) = updater.check().await? else {
        return Ok(false);
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
