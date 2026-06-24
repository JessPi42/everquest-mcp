mod log_parser;
mod sync_engine;

use log_parser::{LogParser, ParsedEvent};
use sync_engine::SyncEngine;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_updater::UpdaterExt;
use tauri_plugin_store::StoreExt;

// ---------------------------------------------------------------------------
// Embedded tray icons (16x16 solid-colour placeholders).
// Replace with production .ico / .png assets at build time.
// ---------------------------------------------------------------------------
static ICON_IDLE: &[u8] = include_bytes!("../icons/tray-idle.png");
static ICON_WATCHING: &[u8] = include_bytes!("../icons/tray-watching.png");
static ICON_SYNCING: &[u8] = include_bytes!("../icons/tray-syncing.png");
static ICON_ERROR: &[u8] = include_bytes!("../icons/tray-error.png");

const STORE_PATH: &str = "eq-partner-settings.json";
const KEY_API_KEY: &str = "apiKey";
const KEY_LOG_PATH: &str = "logFilePath";
const KEY_API_BASE_URL: &str = "apiBaseUrl";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub api_key: String,
    pub log_file_path: String,
    pub api_base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TrayStatus {
    Idle,
    Watching,
    Syncing,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub watching: bool,
    pub tray_status: TrayStatus,
    pub last_sync: Option<String>,
    pub pending_events: usize,
    pub last_error: Option<String>,
    pub connected: bool,
    pub current_zone: Option<String>,
}

struct WatcherState {
    _watcher: Option<RecommendedWatcher>,
    watching: bool,
    current_zone: Option<String>,
    /// Set to `true` to signal the async drain task to exit on its next iteration.
    cancel_flag: Arc<AtomicBool>,
}

struct AppState {
    sync_engine: Option<SyncEngine>,
    watcher: Arc<Mutex<WatcherState>>,
    connected: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            sync_engine: None,
            watcher: Arc::new(Mutex::new(WatcherState {
                _watcher: None,
                watching: false,
                current_zone: None,
                cancel_flag: Arc::new(AtomicBool::new(false)),
            })),
            connected: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Tray helpers
// ---------------------------------------------------------------------------

fn tray_icon_bytes(watching: bool, pending: bool, has_error: bool) -> &'static [u8] {
    if has_error {
        ICON_ERROR
    } else if pending {
        ICON_SYNCING
    } else if watching {
        ICON_WATCHING
    } else {
        ICON_IDLE
    }
}

fn tray_tooltip(watching: bool, zone: Option<&str>, last_error: Option<&str>) -> String {
    if let Some(err) = last_error {
        format!("EQ Partner - Error: {}", &err[..err.len().min(60)])
    } else if watching {
        match zone {
            Some(z) => format!("EQ Partner - Watching ({})", z),
            None => "EQ Partner - Watching".to_string(),
        }
    } else {
        "EQ Partner - Idle".to_string()
    }
}

/// Rebuild the tray menu and update icon/tooltip to reflect current state.
fn refresh_tray(
    app: &AppHandle,
    watching: bool,
    zone: Option<&str>,
    last_error: Option<&str>,
    pending_events: bool,
) {
    let toggle_label = if watching { "Stop Watching" } else { "Start Watching" };

    let Ok(show_item) = MenuItemBuilder::with_id("show", "Open Settings").build(app) else { return; };
    let Ok(toggle_item) = MenuItemBuilder::with_id("toggle", toggle_label).build(app) else { return; };
    let Ok(select_item) = MenuItemBuilder::with_id("select_log", "Select Log File\u{2026}").build(app) else { return; };
    let Ok(separator) = tauri::menu::PredefinedMenuItem::separator(app) else { return; };
    let Ok(quit_item) = MenuItemBuilder::with_id("quit", "Quit").build(app) else { return; };

    let Ok(menu) = MenuBuilder::new(app)
        .item(&show_item)
        .item(&toggle_item)
        .item(&select_item)
        .item(&separator)
        .item(&quit_item)
        .build()
    else {
        return;
    };

    if let Some(tray) = app.tray_by_id("main") {
        let tooltip = tray_tooltip(watching, zone, last_error);
        let icon_bytes = tray_icon_bytes(watching, pending_events, last_error.is_some());
        if let Ok(icon) = tauri::image::Image::from_bytes(icon_bytes) {
            let _ = tray.set_icon(Some(icon));
        }
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_tooltip(Some(tooltip.as_str()));
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn load_settings(app: AppHandle) -> Result<Settings, String> {
    let store = app.store(STORE_PATH).map_err(|e| e.to_string())?;
    let api_key = store.get(KEY_API_KEY).and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    let log_file_path = store.get(KEY_LOG_PATH).and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    let api_base_url = store.get(KEY_API_BASE_URL).and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    Ok(Settings { api_key, log_file_path, api_base_url })
}

#[tauri::command]
async fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let store = app.store(STORE_PATH).map_err(|e| e.to_string())?;
    store.set(KEY_API_KEY, serde_json::Value::String(settings.api_key.clone()));
    store.set(KEY_LOG_PATH, serde_json::Value::String(settings.log_file_path.clone()));
    store.set(KEY_API_BASE_URL, serde_json::Value::String(settings.api_base_url.clone()));
    store.save().map_err(|e| e.to_string())?;

    let engine_opt = {
        let state: State<Arc<Mutex<AppState>>> = app.state();
        let st = state.lock().unwrap();
        st.sync_engine.clone()
    };
    if let Some(engine) = engine_opt {
        engine.update_credentials(settings.api_key, settings.api_base_url).await;
    }

    Ok(())
}

#[tauri::command]
async fn test_connection(api_key: String, api_base_url: String) -> Result<serde_json::Value, String> {
    let url = format!("{}/partner/me", api_base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    match client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({}));
            let username = body["user"]["username"].as_str().unwrap_or("").to_string();
            Ok(serde_json::json!({ "ok": true, "username": username }))
        }
        Ok(resp) => {
            let status = resp.status().as_u16();
            Ok(serde_json::json!({ "ok": false, "error": format!("HTTP {}", status) }))
        }
        Err(e) => Ok(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[tauri::command]
async fn get_status(app: AppHandle) -> AppStatus {
    let (watching, current_zone, connected, engine_opt) = {
        let state = app.state::<Arc<Mutex<AppState>>>();
        let st = state.lock().unwrap();
        let watching = st.watcher.lock().unwrap().watching;
        let current_zone = st.watcher.lock().unwrap().current_zone.clone();
        let connected = st.connected;
        let engine_opt = st.sync_engine.clone();
        (watching, current_zone, connected, engine_opt)
    };

    if let Some(engine) = engine_opt {
        let pending_events = engine.pending_count().await;
        let last_sync = engine.last_sync.lock().await.clone();
        let last_error = engine.last_error.lock().await.clone();

        let tray_status = if last_error.is_some() {
            TrayStatus::Error
        } else if pending_events > 0 {
            TrayStatus::Syncing
        } else if watching {
            TrayStatus::Watching
        } else {
            TrayStatus::Idle
        };

        // Refresh tray icon/tooltip based on live status
        refresh_tray(
            &app,
            watching,
            current_zone.as_deref(),
            last_error.as_deref(),
            pending_events > 0,
        );

        AppStatus {
            watching,
            tray_status,
            last_sync,
            pending_events,
            last_error,
            connected,
            current_zone,
        }
    } else {
        AppStatus {
            watching,
            tray_status: TrayStatus::Idle,
            last_sync: None,
            pending_events: 0,
            last_error: None,
            connected,
            current_zone,
        }
    }
}

#[tauri::command]
async fn start_watching(app: AppHandle, log_path: String) -> Result<(), String> {
    // Cancel any existing watcher before starting a new one
    {
        let state: State<Arc<Mutex<AppState>>> = app.state();
        let st = state.lock().unwrap();
        let ws = st.watcher.lock().unwrap();
        ws.cancel_flag.store(true, Ordering::Relaxed);
    }

    let settings = load_settings(app.clone()).await?;

    let engine = SyncEngine::new(settings.api_key.clone(), settings.api_base_url.clone());
    let engine_clone = engine.clone();
    engine_clone.start_background_sync();

    let path = PathBuf::from(&log_path);
    if !path.exists() {
        return Err(format!("Log file not found: {}", log_path));
    }

    let initial_pos = {
        let f = File::open(&path).map_err(|e| e.to_string())?;
        f.metadata().map_err(|e| e.to_string())?.len()
    };

    let pos = Arc::new(Mutex::new(initial_pos));
    let parser = Arc::new(Mutex::new(LogParser::new(extract_char_name(&log_path))));
    let cancel_flag = Arc::new(AtomicBool::new(false));

    let (tx, rx) = std::sync::mpsc::channel::<()>();

    let engine_for_task = engine.clone();
    let path_for_task = path.clone();
    let pos_for_task = pos.clone();
    let parser_for_task = parser.clone();
    let app_for_task = app.clone();
    let cancel_for_task = cancel_flag.clone();

    // Async task: drain new log lines on each watcher notification.
    // Uses read_line() for CRLF-safe byte tracking (Windows EQ logs use \r\n).
    // Exits when cancel_flag is set OR when tx is dropped (watcher dropped).
    tokio::spawn(async move {
        while rx.recv().is_ok() {
            if cancel_for_task.load(Ordering::Relaxed) {
                break;
            }
            if let Ok(mut file) = File::open(&path_for_task) {
                // Collect new events inside a block so the MutexGuard on
                // current_pos is dropped before any .await calls below.
                let new_events: Vec<ParsedEvent> = {
                    let mut current_pos = pos_for_task.lock().unwrap();
                    if file.seek(SeekFrom::Start(*current_pos)).is_err() {
                        continue;
                    }
                    let mut reader = BufReader::new(&file);
                    let mut new_pos = *current_pos;
                    let mut events: Vec<ParsedEvent> = Vec::new();

                    loop {
                        let mut raw_line = String::new();
                        match reader.read_line(&mut raw_line) {
                            Ok(0) => break, // EOF
                            Ok(bytes_read) => {
                                new_pos += bytes_read as u64;
                                let trimmed = raw_line
                                    .trim_end_matches(|c: char| c == '\r' || c == '\n');
                                let mut p = parser_for_task.lock().unwrap();
                                if let Some(ev) = p.parse_line(trimmed) {
                                    if ev.event_type == "zone" {
                                        if let Some(zone) = ev.zone.clone() {
                                            let state: State<Arc<Mutex<AppState>>> =
                                                app_for_task.state();
                                            {
                                                let st = state.lock().unwrap();
                                                st.watcher.lock().unwrap().current_zone =
                                                    Some(zone.clone());
                                            }
                                            refresh_tray(
                                                &app_for_task,
                                                true,
                                                Some(&zone),
                                                None,
                                                false,
                                            );
                                        }
                                    }
                                    events.push(ev);
                                }
                            }
                            Err(_) => break,
                        }
                    }

                    *current_pos = new_pos;
                    events
                }; // current_pos MutexGuard dropped here

                if !new_events.is_empty() {
                    refresh_tray(&app_for_task, true, None, None, true);
                }
                for ev in new_events {
                    engine_for_task.enqueue(ev).await;
                }
                // After flushing queue, refresh tray back to watching state
                refresh_tray(&app_for_task, true, None, None, false);
            }
        }
    });

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Modify(_)) {
                let _ = tx.send(());
            }
        }
    })
    .map_err(|e| e.to_string())?;

    watcher
        .watch(&path, RecursiveMode::NonRecursive)
        .map_err(|e| e.to_string())?;

    let state: State<Arc<Mutex<AppState>>> = app.state();
    let mut st = state.lock().unwrap();
    st.sync_engine = Some(engine);
    {
        let mut ws = st.watcher.lock().unwrap();
        ws._watcher = Some(watcher);
        ws.watching = true;
        ws.cancel_flag = cancel_flag;
    }
    drop(st);

    refresh_tray(&app, true, None, None, false);
    Ok(())
}

#[tauri::command]
async fn stop_watching(app: AppHandle) -> Result<(), String> {
    let state: State<Arc<Mutex<AppState>>> = app.state();
    let mut st = state.lock().unwrap();
    {
        let mut ws = st.watcher.lock().unwrap();
        // Signal the drain task to exit on its next check
        ws.cancel_flag.store(true, Ordering::Relaxed);
        ws._watcher = None; // drops watcher → drops tx → rx.recv() fails → task exits
        ws.watching = false;
        ws.current_zone = None;
        // Replace flag so next start_watching gets a fresh one
        ws.cancel_flag = Arc::new(AtomicBool::new(false));
    }
    st.sync_engine = None;
    drop(st);

    refresh_tray(&app, false, None, None, false);
    Ok(())
}

/// Re-check for the latest update and apply it.  Called from the frontend
/// "Install" button.  On Windows / NSIS the installer runs and the old process
/// exits, so this command may not return a response to the caller.
///
/// Emits `update-progress` events with `{ downloaded, total }` (bytes) on
/// each downloaded chunk so the frontend can display a real progress bar.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No update available".to_string())?;

    let app_for_progress = app.clone();
    let app_for_finish = app.clone();
    let mut downloaded: u64 = 0;

    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = app_for_progress.emit(
                    "update-progress",
                    serde_json::json!({ "downloaded": downloaded, "total": total }),
                );
            },
            move || {
                let _ = app_for_finish.emit("update-installing", ());
            },
        )
        .await
        .map_err(|e| e.to_string())
}

fn extract_char_name(log_path: &str) -> Option<String> {
    let filename = std::path::Path::new(log_path).file_stem()?.to_str()?;
    let parts: Vec<&str> = filename.split('_').collect();
    if parts.len() >= 2 && parts[0] == "eqlog" {
        Some(parts[1].to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tray setup
// ---------------------------------------------------------------------------

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItemBuilder::with_id("show", "Open Settings").build(app)?;
    let toggle_item = MenuItemBuilder::with_id("toggle", "Start Watching").build(app)?;
    let select_item =
        MenuItemBuilder::with_id("select_log", "Select Log File\u{2026}").build(app)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&show_item)
        .item(&toggle_item)
        .item(&select_item)
        .item(&separator)
        .item(&quit_item)
        .build()?;

    let icon = tauri::image::Image::from_bytes(ICON_IDLE)
        .unwrap_or_else(|_| tauri::image::Image::new(&[], 0, 0));

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("EQ Partner - Idle")
        .menu(&menu)
        .on_menu_event(|app, event| {
            match event.id().as_ref() {
                "show" => {
                    if let Some(window) = app.get_webview_window("settings") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "toggle" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let state: State<Arc<Mutex<AppState>>> = app.state();
                        let is_watching =
                            state.lock().unwrap().watcher.lock().unwrap().watching;
                        if is_watching {
                            let _ = stop_watching(app.clone()).await;
                        } else {
                            match load_settings(app.clone()).await {
                                Ok(s) if !s.log_file_path.is_empty() => {
                                    if let Err(e) =
                                        start_watching(app.clone(), s.log_file_path).await
                                    {
                                        refresh_tray(&app, false, None, Some(&e), false);
                                        if let Some(w) = app.get_webview_window("settings") {
                                            let _ = w.show();
                                            let _ = w.set_focus();
                                        }
                                    }
                                }
                                _ => {
                                    if let Some(w) = app.get_webview_window("settings") {
                                        let _ = w.show();
                                        let _ = w.set_focus();
                                    }
                                }
                            }
                        }
                    });
                }
                "select_log" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        use tauri_plugin_dialog::DialogExt;
                        let picked = app
                            .dialog()
                            .file()
                            .add_filter("EQ Log Files", &["txt"])
                            .blocking_pick_file();

                        if let Some(file_path) = picked {
                            let path_str = file_path.to_string();
                            if let Ok(mut settings) = load_settings(app.clone()).await {
                                settings.log_file_path = path_str.clone();
                                let _ = save_settings(app.clone(), settings).await;
                            }
                            if let Err(e) = start_watching(app.clone(), path_str).await {
                                refresh_tray(&app, false, None, Some(&e), false);
                                if let Some(w) = app.get_webview_window("settings") {
                                    let _ = w.show();
                                    let _ = w.set_focus();
                                }
                            }
                        }
                    });
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("settings") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(Arc::new(Mutex::new(AppState::default())))
        .setup(|app| {
            setup_tray(&app.handle())?;

            // Start hidden — visible only via tray icon
            if let Some(window) = app.get_webview_window("settings") {
                let _ = window.hide();
            }

            // Check for updates in the background on launch.
            // Emits "update-available" to the frontend if a newer version is found.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(updater) = handle.updater() {
                    if let Ok(Some(update)) = updater.check().await {
                        let _ = handle.emit("update-available", update.version.clone());
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            test_connection,
            get_status,
            start_watching,
            stop_watching,
            install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running EQ Partner");
}
