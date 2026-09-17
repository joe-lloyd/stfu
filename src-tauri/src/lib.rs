mod audio;
mod commands;
mod config;
mod hotkey;
mod inject;
mod lang;
mod llm;
#[cfg(target_os = "macos")]
mod permissions;
mod stt;
mod updater;

use audio::Recorder;
use config::Config;
use hotkey::HotkeyEvent;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::time::{Duration, Instant};
use tauri::{
    menu::{CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
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
            Err(_) => (190.0 * scale, 40.0 * scale),
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
    builder.format_timestamp_millis();
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::config_path,
            commands::save_config,
            commands::test_stt,
            commands::test_llm,
            commands::open_url,
            commands::languages,
            commands::zen_models,
            commands::import_opencode_key,
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
            // Menu bar / tray app: no Dock icon, no app switcher entry. Tauri sets the activation
            // policy at runtime, so LSUIElement in Info.plist alone is not enough.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Launch at login: make the OS registration match the config on every start, so the
            // setting follows the config file (and survives the app being moved by the updater).
            {
                use tauri_plugin_autostart::ManagerExt;
                let auto = app.autolaunch();
                let result = if config.launch_at_login { auto.enable() } else { auto.disable() };
                match result {
                    Ok(()) => log::info!("launch at login: {}", config.launch_at_login),
                    Err(e) => log::warn!("could not update launch-at-login registration: {e}"),
                }
            }

            // Language submenu: switch dictation language without opening Settings.
            let current_lang = config.stt.language.trim().to_string();
            let lang_items: Vec<CheckMenuItem<_>> = lang::LANGUAGES
                .iter()
                .map(|(code, label)| {
                    CheckMenuItemBuilder::with_id(format!("lang:{code}"), *label)
                        .checked(*code == current_lang)
                        .build(app)
                })
                .collect::<tauri::Result<_>>()?;
            let lang_refs: Vec<&dyn tauri::menu::IsMenuItem<_>> =
                lang_items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<_>).collect();
            let language_menu = SubmenuBuilder::new(app, format!("Language: {}", lang::label(&current_lang)))
                .items(&lang_refs)
                .build()?;

            app.manage(commands::LangMenu {
                items: lang_items.clone(),
                submenu: language_menu.clone(),
            });

            // Tray with Settings, Language, Launch at login, Check for updates and Quit; the app has no main window.
            let settings = MenuItemBuilder::with_id("settings", "Settings…").build(app)?;
            let autostart = CheckMenuItemBuilder::with_id("autostart", "Launch at login")
                .checked(config.launch_at_login)
                .build(app)?;
            let update = MenuItemBuilder::with_id("update", "Check for updates").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit stfu").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&settings])
                .item(&language_menu)
                .items(&[&autostart])
                .separator()
                .items(&[&update, &quit])
                .build()?;
            let autostart_item = autostart.clone();
            let shared_for_tray = shared.clone();
            let lang_items_for_tray = lang_items.clone();
            let language_menu_for_tray = language_menu.clone();
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip(format!("stfu {} — hold {} to dictate", env!("CARGO_PKG_VERSION"), config.hotkey.join("+")))
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "settings" => show_settings(app),
                    "update" => updater::check_now(app.clone()),
                    id if id.starts_with("lang:") => {
                        let code = id.trim_start_matches("lang:").to_string();
                        // Radio behaviour: the clicked item wins, every other one clears.
                        for (item, (c, _)) in lang_items_for_tray.iter().zip(lang::LANGUAGES) {
                            let _ = item.set_checked(*c == code);
                        }
                        let _ = language_menu_for_tray
                            .set_text(format!("Language: {}", lang::label(&code)));
                        let mut cfg = shared_for_tray.write().unwrap();
                        cfg.stt.language = code.clone();
                        match cfg.save() {
                            Ok(()) => log::info!("dictation language set to {:?}", code),
                            Err(e) => log::warn!("could not save config: {e:#}"),
                        }
                    }
                    "autostart" => {
                        use tauri_plugin_autostart::ManagerExt;
                        let enable = autostart_item.is_checked().unwrap_or(true);
                        let auto = app.autolaunch();
                        let result = if enable { auto.enable() } else { auto.disable() };
                        match result {
                            Ok(()) => {
                                log::info!("launch at login set to {enable}");
                                let mut cfg = shared_for_tray.write().unwrap();
                                cfg.launch_at_login = enable;
                                if let Err(e) = cfg.save() {
                                    log::warn!("could not save config: {e:#}");
                                }
                            }
                            Err(e) => {
                                log::error!("could not change launch at login: {e}");
                                let _ = autostart_item.set_checked(!enable);
                            }
                        }
                    }
                    _ => {}
                });
            #[cfg(target_os = "macos")]
            {
                // Monochrome template image: macOS tints it for light/dark menu bars.
                let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template@2x.png"))?;
                tray = tray.icon(icon).icon_as_template(true);
            }
            #[cfg(not(target_os = "macos"))]
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // Background auto-update: first check shortly after launch, then every few hours.
            updater::start(app.handle().clone());

            #[cfg(target_os = "macos")]
            {
                let trusted = permissions::accessibility_trusted();
                log::info!("accessibility trusted: {trusted}");
                if !trusted {
                    log::warn!("requesting Accessibility permission; will restart once granted");
                    permissions::request_accessibility();
                    // macOS only applies a new Accessibility grant to a fresh process, and until
                    // then synthetic keystrokes are dropped. Poll and restart ourselves when it lands.
                    let handle = app.handle().clone();
                    std::thread::spawn(move || loop {
                        std::thread::sleep(Duration::from_secs(2));
                        if permissions::accessibility_trusted() {
                            log::info!("Accessibility granted; restarting to apply it");
                            handle.restart();
                        }
                    });
                }
            }

            // Ask macOS for microphone access explicitly. CoreAudio alone does not trigger the
            // system prompt, and without a grant macOS silently delivers all-zero audio.
            #[cfg(target_os = "macos")]
            permissions::request_microphone(|granted| {
                if granted {
                    log::info!("microphone access granted");
                } else {
                    log::error!("microphone access denied: enable stfu under System Settings > Privacy & Security > Microphone");
                    let _ = std::process::Command::new("open")
                        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
                        .spawn();
                }
            });

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
                log::info!("hotkey down");
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
                log::info!("hotkey up");
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
                    Ok(w) if w.is_empty() => {
                        // Nothing usable was recorded: hide the pill, no error.
                        if let Some(win) = pill(&app) {
                            let _ = win.hide();
                        }
                        continue;
                    }
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

/// Returns `Ok(None)` when there was nothing worth pasting (no speech), so the caller stays quiet.
async fn process(
    app: &AppHandle,
    cfg: &Config,
    client: &reqwest::Client,
    wav: Vec<u8>,
) -> anyhow::Result<Option<String>> {
    let t0 = Instant::now();
    let raw = stt::transcribe(client, cfg, wav).await?;
    log::info!("stt {:?}: {raw:?}", t0.elapsed());
    if !raw.chars().any(|c| c.is_alphanumeric()) {
        log::info!("no speech in transcript, skipping");
        return Ok(None);
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

    if !text.chars().any(|c| c.is_alphanumeric()) {
        log::info!("cleanup produced no text, skipping");
        return Ok(None);
    }

    set_state(app, "processing", Some("Pasting…".into()));
    let to_paste = text.clone();
    tokio::task::spawn_blocking(move || inject::paste(&to_paste)).await??;
    log::info!("total {:?}", t0.elapsed());
    Ok(Some(text))
}

fn finish(app: &AppHandle, result: anyhow::Result<Option<String>>) {
    let hide_after = match result {
        Ok(None) => Duration::ZERO,
        Ok(Some(_)) => {
            set_state(app, "done", None);
            Duration::from_millis(1000) // ripple + fade-out play inside the webview
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
