mod audio;
mod commands;
mod config;
mod hotkey;
mod inject;
mod llm;
mod stt;

use audio::Recorder;
use config::Config;
use hotkey::HotkeyEvent;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::time::{Duration, Instant};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow,
};

const PILL: &str = "pill";
const SETTINGS: &str = "settings";

pub type SharedConfig = Arc<RwLock<Config>>;

#[derive(Serialize, Clone)]
struct StateEvent<'a> {
    phase: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

fn set_state(app: &AppHandle, phase: &str, message: Option<String>) {
    let _ = app.emit_to(PILL, "state", StateEvent { phase, message });
}

fn pill(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(PILL)
}

fn show_settings(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(SETTINGS) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// Bottom-centre of the primary monitor, a little above the dock/taskbar.
fn position_pill(win: &WebviewWindow) {
    if let Ok(Some(monitor)) = win.primary_monitor() {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let (w, h) = match win.outer_size() {
            Ok(s) => (s.width as f64, s.height as f64),
            Err(_) => (260.0 * scale, 48.0 * scale),
        };
        let x = monitor.position().x as f64 + (size.width as f64 - w) / 2.0;
        let y = monitor.position().y as f64 + size.height as f64 - h - 90.0 * scale;
        let _ = win.set_position(PhysicalPosition::new(x as i32, y as i32));
    }
}

/// Logs to stderr and to `stfu.log` next to the config file, so a bundled app (no terminal) is debuggable.
fn init_logging() {
    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    if let Ok(path) = Config::path() {
        let log_path = path.with_file_name("stfu.log");
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            struct Tee(std::fs::File);
            impl std::io::Write for Tee {
                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    let _ = std::io::stderr().write_all(buf);
                    self.0.write(buf)
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    self.0.flush()
                }
            }
            builder.target(env_logger::Target::Pipe(Box::new(Tee(file))));
        }
    }
    builder.init();
}

pub fn run() {
    init_logging();

    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            log::error!("config error: {e:#}");
            Config::default()
        }
    };
    log::info!(
        "config: {} | hotkey {:?} | stt {} @ {} | llm {} @ {} (enabled={})",
        Config::path().map(|p| p.display().to_string()).unwrap_or_default(),
        config.hotkey,
        config.stt.model,
        config.stt.base_url,
        config.llm.model,
        config.llm.base_url,
        config.llm.enabled
    );

    let shared: SharedConfig = Arc::new(RwLock::new(config.clone()));

    tauri::Builder::default()
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::config_path,
            commands::save_config,
            commands::test_stt,
            commands::test_llm,
            commands::open_url,
        ])
        .on_window_event(|window, event| {
            // Closing the settings window hides it so it can be reopened from the tray.
            if window.label() == SETTINGS {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(move |app| {
            // Tray with Settings and Quit, since the app has no main window.
            let settings = MenuItemBuilder::with_id("settings", "Settings…").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit stfu").build(app)?;
            let menu = MenuBuilder::new(app).items(&[&settings, &quit]).build()?;
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip(format!("stfu — hold {} to dictate", config.hotkey.join("+")))
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "settings" => show_settings(app),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // First run or missing keys: open settings straight away.
            if config.needs_setup() {
                show_settings(&app.handle().clone());
            }

            // Global hotkey listener.
            let keys = config
                .hotkey
                .iter()
                .map(|k| hotkey::parse_key(k))
                .collect::<anyhow::Result<Vec<_>>>()
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            let (tx, rx) = mpsc::channel::<HotkeyEvent>();
            hotkey::spawn(keys, tx);

            let recorder = Recorder::spawn();
            let handle = app.handle().clone();
            let cfg = shared.clone();
            std::thread::Builder::new()
                .name("pipeline".into())
                .spawn(move || pipeline_loop(handle, cfg, recorder, rx))
                .expect("spawn pipeline thread");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Owns the record -> transcribe -> clean -> paste flow. One dictation at a time.
fn pipeline_loop(
    app: AppHandle,
    cfg: SharedConfig,
    recorder: Recorder,
    rx: mpsc::Receiver<HotkeyEvent>,
) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let client = reqwest::Client::new();
    let busy = Arc::new(AtomicBool::new(false));
    let mut recording = false;
    let mut started_at = Instant::now();

    while let Ok(ev) = rx.recv() {
        match ev {
            HotkeyEvent::Down => {
                if busy.load(Ordering::SeqCst) || recording {
                    continue;
                }
                match recorder.start() {
                    Ok(levels) => {
                        recording = true;
                        started_at = Instant::now();
                        if let Some(win) = pill(&app) {
                            position_pill(&win);
                            let _ = win.show();
                        }
                        set_state(&app, "recording", None);
                        let app2 = app.clone();
                        std::thread::spawn(move || {
                            while let Ok(level) = levels.recv() {
                                let _ = app2.emit_to(PILL, "level", level);
                            }
                        });
                    }
                    Err(e) => log::error!("start recording: {e:#}"),
                }
            }
            HotkeyEvent::Up => {
                if !recording {
                    continue;
                }
                recording = false;
                let wav = recorder.stop();
                // Taps shorter than this are almost always accidental.
                if started_at.elapsed() < Duration::from_millis(300) {
                    if let Some(win) = pill(&app) {
                        let _ = win.hide();
                    }
                    continue;
                }
                let wav = match wav {
                    Ok(w) => w,
                    Err(e) => {
                        finish(&app, Err(e));
                        continue;
                    }
                };
                // Keep the most recent take next to the config for debugging ("what did it hear?").
                if let Ok(path) = Config::path() {
                    let _ = std::fs::write(path.with_file_name("last.wav"), &wav);
                }
                busy.store(true, Ordering::SeqCst);
                set_state(&app, "processing", Some("Transcribing…".into()));

                let snapshot = cfg.read().unwrap().clone();
                if snapshot.needs_setup() {
                    show_settings(&app);
                }
                let (app2, client2, busy2) = (app.clone(), client.clone(), busy.clone());
                rt.spawn(async move {
                    let result = process(&app2, &snapshot, &client2, wav).await;
                    finish(&app2, result);
                    busy2.store(false, Ordering::SeqCst);
                });
            }
        }
    }
}

async fn process(
    app: &AppHandle,
    cfg: &Config,
    client: &reqwest::Client,
    wav: Vec<u8>,
) -> anyhow::Result<String> {
    let t0 = Instant::now();
    let raw = stt::transcribe(client, cfg, wav).await?;
    log::info!("stt {:?}: {raw:?}", t0.elapsed());
    if raw.is_empty() {
        anyhow::bail!("nothing heard");
    }

    let text = if cfg.llm.enabled {
        set_state(app, "processing", Some("Cleaning up…".into()));
        let t1 = Instant::now();
        match llm::cleanup(client, cfg, &raw).await {
            Ok(clean) => {
                log::info!("llm {:?}: {clean:?}", t1.elapsed());
                clean
            }
            Err(e) => {
                log::warn!("cleanup failed, pasting raw transcript: {e:#}");
                raw.clone()
            }
        }
    } else {
        raw.clone()
    };

    set_state(app, "processing", Some("Pasting…".into()));
    let to_paste = text.clone();
    tokio::task::spawn_blocking(move || inject::paste(&to_paste)).await??;
    log::info!("total {:?}", t0.elapsed());
    Ok(text)
}

fn finish(app: &AppHandle, result: anyhow::Result<String>) {
    let hide_after = match result {
        Ok(_) => {
            set_state(app, "done", None);
            Duration::from_millis(600)
        }
        Err(e) => {
            log::error!("dictation failed: {e:#}");
            let short: String = e.to_string().chars().take(60).collect();
            set_state(app, "error", Some(short));
            Duration::from_millis(2500)
        }
    };
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(hide_after);
        if let Some(win) = pill(&app) {
            let _ = win.hide();
        }
    });
}
