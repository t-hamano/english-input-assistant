mod config;
mod keyboard;
mod llm;
mod tts;
mod updater;
mod window_manager;

macro_rules! debug_log {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        println!($($arg)*);
    };
}

use std::str::FromStr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{thread, time::Duration};

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use config::{ConfigStore, SettingsConfig};
use llm::TranslationResult;
use tts::{AudioCache, StopSignal};

/// State shared between shortcut handler and commands.
struct AppState {
    translation_id: AtomicU64,
    selected_text: Mutex<String>,
    /// Source window handle to restore focus after paste (Windows).
    #[cfg(target_os = "windows")]
    source_hwnd: Mutex<usize>,
    /// Source application PID to restore focus after paste (macOS).
    #[cfg(target_os = "macos")]
    source_pid: Mutex<i32>,
    audio_cache: AudioCache,
    config_store: ConfigStore,
    tts_stop: Mutex<Option<StopSignal>>,
    tts_window: Mutex<Option<String>>,
}

/// Call LLM with automatic retry (up to 3 attempts).
fn translate_with_retry(
    app: &AppHandle,
    request_id: u64,
    input: &str,
    api_key: &str,
    model: &str,
    additional_prompt: &str,
) -> Result<TranslationResult, String> {
    let max_retries = 3;
    let mut displayed = false;
    let is_current = || app.state::<AppState>().translation_id.load(Ordering::SeqCst) == request_id;

    for attempt in 1..=max_retries {
        if !is_current() {
            return Err("Translation cancelled".to_string());
        }
        match llm::translate(api_key, model, input, additional_prompt, |result| {
            displayed = true;
            emit_translation(app, request_id, &result, false);
        }, is_current) {
            Ok(result) => return Ok(result),
            Err(e) => {
                debug_log!(
                    "[rust] LLM attempt {}/{} failed: {}",
                    attempt, max_retries, e
                );
                // Do not replace an English sentence the user may already be using.
                if displayed || !is_current() {
                    return Err(e);
                }
                if attempt < max_retries {
                    thread::sleep(Duration::from_millis(500 * attempt as u64));
                } else {
                    return Err(e);
                }
            }
        }
    }
    unreachable!()
}

fn emit_translation(app: &AppHandle, request_id: u64, result: &TranslationResult, complete: bool) {
    if app.state::<AppState>().translation_id.load(Ordering::SeqCst) == request_id {
        let _ = app.emit("translation-update", serde_json::json!({
            "request_id": request_id, "result": result, "complete": complete,
        }));
    }
}

fn run_translation(app: &AppHandle, request_id: u64, input: &str) {
    let (api_key, model, additional_prompt) = match get_llm_config(app) {
        Ok(config) => config,
        Err(message) => {
            if app.state::<AppState>().translation_id.load(Ordering::SeqCst) == request_id {
                let _ = app.emit("translation-error", serde_json::json!({
                    "request_id": request_id, "message": message,
                }));
            }
            return;
        }
    };
    if api_key.is_empty() {
        emit_translation(app, request_id, &TranslationResult {
            translated: format!("[Translated] {}", input),
            explanation: "N/A (no API key set)".to_string(),
            source_is_english: false,
        }, true);
        return;
    }
    match translate_with_retry(app, request_id, input, &api_key, &model, &additional_prompt) {
        Ok(result) => emit_translation(app, request_id, &result, true),
        Err(message) => {
            if app.state::<AppState>().translation_id.load(Ordering::SeqCst) == request_id {
                let _ = app.emit("translation-error", serde_json::json!({
                    "request_id": request_id, "message": message,
                }));
            }
        }
    }
}

fn get_llm_config(app: &AppHandle) -> Result<(String, String, String), String> {
    let config = app.state::<AppState>().config_store.get();
    Ok((app.state::<AppState>().config_store.api_key()?, config.gemini_model, config.additional_prompt))
}

fn show_popup(app: &AppHandle, x: i32, y: i32) {
    if let Some(win) = app.get_webview_window("main") {
        // Get initial size from tauri.conf.json window config
        let win_config = app.config().app.windows.first();
        let popup_width = win_config.map(|w| w.width as i32).unwrap_or(520);
        let popup_height = win_config.map(|w| w.height as i32).unwrap_or(300);

        // Get monitor info for the cursor position
        let monitor = win.available_monitors().ok().and_then(|monitors| {
            monitors.into_iter().find(|m| {
                let pos = m.position();
                let size = m.size();
                x >= pos.x
                    && x < pos.x + size.width as i32
                    && y >= pos.y
                    && y < pos.y + size.height as i32
            })
        });

        let (final_x, final_y) = if let Some(monitor) = monitor {
            let m_pos = monitor.position();
            let m_size = monitor.size();
            let screen_bottom = m_pos.y + m_size.height as i32;
            let screen_right = m_pos.x + m_size.width as i32;
            // Cursor and monitor coordinates are physical; config sizes are logical.
            let physical_width = (popup_width as f64 * monitor.scale_factor()).round() as i32;
            let physical_height = (popup_height as f64 * monitor.scale_factor()).round() as i32;

            let fx = if x + physical_width > screen_right {
                x - physical_width
            } else {
                x
            };
            let fy = if y + physical_height > screen_bottom {
                y - physical_height
            } else {
                y + 20
            };
            (fx.max(m_pos.x), fy.max(m_pos.y))
        } else {
            (x, y + 20)
        };

        let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            final_x, final_y,
        )));
        let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            popup_width as f64,
            popup_height as f64,
        )));
        let _ = win.set_always_on_top(true);
        let _ = win.set_focus();
        let _ = win.show();
    }
}

fn on_shortcut(app: AppHandle) {
    debug_log!("[rust] shortcut triggered");
    let request_id = app.state::<AppState>().translation_id.fetch_add(1, Ordering::SeqCst) + 1;

    let (cx, cy) = keyboard::get_cursor_position();

    // --- Windows: track source window (HWND) ---
    #[cfg(target_os = "windows")]
    let source_hwnd: usize = {
        let fg_hwnd = window_manager::get_foreground_window();

        // Collect HWNDs of all our own windows
        let own_hwnds: Vec<usize> = ["main", "settings"]
            .iter()
            .filter_map(|label| {
                app.get_webview_window(label)
                    .and_then(|w| w.hwnd().ok())
                    .map(|h| h.0 as usize)
            })
            .collect();

        if own_hwnds.contains(&(fg_hwnd as usize)) {
            // Foreground is one of our windows — use previously stored source window
            app.try_state::<AppState>()
                .map(|s| *s.source_hwnd.lock().unwrap())
                .unwrap_or(0)
        } else {
            // Store this as the source window
            let hwnd = fg_hwnd as usize;
            if let Some(state) = app.try_state::<AppState>() {
                *state.source_hwnd.lock().unwrap() = hwnd;
            }
            hwnd
        }
    };

    // --- macOS: track source application (PID) ---
    #[cfg(target_os = "macos")]
    let source_pid: i32 = {
        let fg_pid = window_manager::get_foreground_pid();
        let own_pid = window_manager::own_pid();

        if fg_pid == own_pid {
            // Our popup is frontmost — use previously stored source pid
            app.try_state::<AppState>()
                .map(|s| *s.source_pid.lock().unwrap())
                .unwrap_or(0)
        } else {
            // Store this as the source application
            if let Some(state) = app.try_state::<AppState>() {
                *state.source_pid.lock().unwrap() = fg_pid;
            }
            fg_pid
        }
    };

    // Hide popup and restore focus to source window
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    #[cfg(target_os = "windows")]
    if source_hwnd != 0 {
        window_manager::set_foreground_window(source_hwnd as *mut _);
    }
    #[cfg(target_os = "macos")]
    if source_pid != 0 {
        window_manager::set_foreground_pid(source_pid);
    }
    thread::sleep(Duration::from_millis(150));

    // Wait for user to release shortcut keys
    thread::sleep(Duration::from_millis(300));
    keyboard::release_modifiers();
    thread::sleep(Duration::from_millis(50));

    // Clear clipboard before cut so we can detect if anything was actually selected
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text("");
    }
    thread::sleep(Duration::from_millis(50));

    keyboard::send_cut();
    debug_log!("[rust] Cut sent");
    thread::sleep(Duration::from_millis(200));

    // Read clipboard
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(_e) => {
            debug_log!("[rust] clipboard error: {}", _e);
            return;
        }
    };

    let selected_text = match clipboard.get_text() {
        Ok(t) => t,
        Err(_e) => {
            debug_log!("[rust] clipboard read error: {}", _e);
            return;
        }
    };
    drop(clipboard);
    debug_log!("[rust] captured: {:?}", selected_text);

    if selected_text.trim().is_empty() {
        debug_log!("[rust] no text selected, aborting");
        return;
    }

    // Limit text length to prevent excessive API usage
    const MAX_TEXT_LENGTH: usize = 5000;
    if selected_text.len() > MAX_TEXT_LENGTH {
        debug_log!("[rust] text too long ({} chars), truncating to {}", selected_text.len(), MAX_TEXT_LENGTH);
        // Restore original text since we won't process it
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(&selected_text);
        }
        keyboard::send_paste();
        let _ = app.emit("show-error", "Text is too long (max 5000 characters). Please select a shorter text.");
        show_popup(&app, cx, cy);
        return;
    }

    // Store selected text for retry
    if let Some(state) = app.try_state::<AppState>() {
        *state.selected_text.lock().unwrap() = selected_text.clone();
    }

    // Show popup with loading state
    let _ = app.emit("show-loading", serde_json::json!({
        "request_id": request_id, "text": selected_text,
    }));
    show_popup(&app, cx, cy);

    run_translation(&app, request_id, &selected_text);
}

#[tauri::command]
fn restore_original_text(app: AppHandle) {
    app.state::<AppState>().translation_id.fetch_add(1, Ordering::SeqCst);
    let text = app
        .state::<AppState>()
        .selected_text
        .lock()
        .unwrap()
        .clone();
    if !text.is_empty() {
        do_paste(text, app);
    }
}

#[tauri::command]
fn do_paste(text: String, app: AppHandle) {
    app.state::<AppState>().translation_id.fetch_add(1, Ordering::SeqCst);
    // Hide popup and restore focus to source window
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }

    #[cfg(target_os = "windows")]
    {
        let source_hwnd = *app.state::<AppState>().source_hwnd.lock().unwrap();
        if source_hwnd != 0 {
            window_manager::set_foreground_window(source_hwnd as *mut _);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let source_pid = *app.state::<AppState>().source_pid.lock().unwrap();
        if source_pid != 0 {
            window_manager::set_foreground_pid(source_pid);
        }
    }
    thread::sleep(Duration::from_millis(150));

    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(_e) => {
            debug_log!("[rust] clipboard error: {}", _e);
            return;
        }
    };
    if let Err(_e) = clipboard.set_text(&text) {
        debug_log!("[rust] clipboard write error: {}", _e);
        return;
    }
    drop(clipboard);
    thread::sleep(Duration::from_millis(100));

    keyboard::release_modifiers();
    thread::sleep(Duration::from_millis(50));
    keyboard::send_paste();
    debug_log!("[rust] pasted: {:?}", text);
}

#[tauri::command]
fn retry_translation(app: AppHandle) {
    let selected_text = app
        .state::<AppState>()
        .selected_text
        .lock()
        .unwrap()
        .clone();

    if selected_text.is_empty() {
        return;
    }

    let request_id = app.state::<AppState>().translation_id.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = app.emit("show-loading", serde_json::json!({
        "request_id": request_id, "text": selected_text,
    }));

    thread::spawn(move || {
        run_translation(&app, request_id, &selected_text);
    });
}

fn stop_current_tts(app: &AppHandle) {
    if let Some(signal) = app.state::<AppState>().tts_stop.lock().unwrap().take() {
        signal.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

fn emit_tts(app: &AppHandle, event: &str) {
    let window = app.state::<AppState>().tts_window.lock().unwrap().clone();
    if let Some(label) = window {
        let _ = app.emit_to(&label, event, ());
    }
}

fn start_tts(app: &AppHandle, window_label: &str) {
    stop_current_tts(app);
    *app.state::<AppState>().tts_window.lock().unwrap() = Some(window_label.to_string());
    emit_tts(app, "tts-playing");
}

#[tauri::command]
async fn play_tts(text: String, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || play_tts_inner(text, app))
        .await.map_err(|_| "Could not start audio playback.".to_string())
}

fn play_tts_inner(text: String, app: AppHandle) {
    start_tts(&app, "main");

    let config = app.state::<AppState>().config_store.get();
    let voice = config.tts_voice.clone();
    let speed = config.tts_speed;

    // Cache key includes voice and speed
    let cache_key = format!("{}|{}|{}", text, voice, speed);

    // Check cache first
    let cached = app.state::<AppState>().audio_cache.get(&cache_key);
    if let Some(audio) = cached {
        debug_log!("[rust] TTS cache hit for: {:?}", text);
        let stop = tts::new_stop_signal();
        *app.state::<AppState>().tts_stop.lock().unwrap() = Some(stop.clone());
        let app_clone = app.clone();
        thread::spawn(move || {
            if let Err(_e) = tts::play_audio(&audio, &stop) {
                debug_log!("[rust] audio playback error: {}", _e);
            }
            emit_tts(&app_clone, "tts-done");
        });
        return;
    }

    let api_key = match app.state::<AppState>().config_store.api_key() {
        Ok(key) => key,
        Err(message) => {
            let _ = app.emit_to("main", "tts-error", message);
            emit_tts(&app, "tts-done");
            return;
        }
    };

    if api_key.is_empty() {
        debug_log!("[rust] No API key set for TTS, skipping");
        emit_tts(&app, "tts-done");
        return;
    }

    let stop = tts::new_stop_signal();
    *app.state::<AppState>().tts_stop.lock().unwrap() = Some(stop.clone());

    let app_clone = app.clone();
    debug_log!("[rust] requesting TTS for: {:?}", text);
    thread::spawn(move || {
        match tts::synthesize(&api_key, &text, &voice, speed) {
            Ok(audio) => {
                debug_log!("[rust] TTS audio received ({} bytes)", audio.len());
                app_clone
                    .state::<AppState>()
                    .audio_cache
                    .insert(cache_key, audio.clone());
                if let Err(_e) = tts::play_audio(&audio, &stop) {
                    debug_log!("[rust] audio playback error: {}", _e);
                }
            }
            Err(_e) => {
                debug_log!("[rust] TTS error: {}", _e);
            }
        }
        emit_tts(&app_clone, "tts-done");
    });
}

#[tauri::command]
fn stop_tts(app: AppHandle) {
    stop_current_tts(&app);
    emit_tts(&app, "tts-done");
}

#[tauri::command]
async fn preview_tts(voice: String, speed: f64, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || preview_tts_inner(voice, speed, app))
        .await.map_err(|_| "Could not start audio playback.".to_string())
}

fn preview_tts_inner(voice: String, speed: f64, app: AppHandle) {
    start_tts(&app, "settings");

    let api_key = match app.state::<AppState>().config_store.api_key() {
        Ok(key) => key,
        Err(message) => {
            let _ = app.emit_to("settings", "tts-error", message);
            emit_tts(&app, "tts-done");
            return;
        }
    };
    if api_key.is_empty() {
        emit_tts(&app, "tts-done");
        return;
    }

    let text = "The quick brown fox jumps over the lazy dog.".to_string();
    let cache_key = format!("{}|{}|{}", text, voice, speed);

    let cached = app.state::<AppState>().audio_cache.get(&cache_key);
    if let Some(audio) = cached {
        let stop = tts::new_stop_signal();
        *app.state::<AppState>().tts_stop.lock().unwrap() = Some(stop.clone());
        let app_clone = app.clone();
        thread::spawn(move || {
            let _ = tts::play_audio(&audio, &stop);
            emit_tts(&app_clone, "tts-done");
        });
        return;
    }

    let stop = tts::new_stop_signal();
    *app.state::<AppState>().tts_stop.lock().unwrap() = Some(stop.clone());

    let app_clone = app.clone();
    thread::spawn(move || {
        if let Ok(audio) = tts::synthesize(&api_key, &text, &voice, speed) {
            app_clone
                .state::<AppState>()
                .audio_cache
                .insert(cache_key, audio.clone());
            let _ = tts::play_audio(&audio, &stop);
        }
        emit_tts(&app_clone, "tts-done");
    });
}

#[tauri::command]
fn get_gemini_models() -> Vec<llm::GeminiModel> {
    llm::ALLOWED_MODELS.to_vec()
}

#[tauri::command]
fn get_tts_voices() -> Vec<tts::TtsVoice> {
    tts::ALLOWED_VOICES.to_vec()
}

#[tauri::command]
async fn get_config(app: AppHandle) -> Result<SettingsConfig, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().config_store.settings())
        .await.map_err(|_| "Could not load settings.".to_string())?
}

#[tauri::command]
async fn save_config(config: SettingsConfig, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || save_settings(config, app))
        .await.map_err(|_| "Could not save settings.".to_string())?
}

fn save_settings(settings: SettingsConfig, app: AppHandle) -> Result<(), String> {
    let config = &settings.preferences;
    // Validate the new shortcut before doing anything else (empty = no shortcut)
    let new_shortcut = if config.shortcut.is_empty() {
        None
    } else {
        Some(
            Shortcut::from_str(&config.shortcut)
                .map_err(|e| format!("Invalid shortcut \"{}\": {}", config.shortcut, e))?,
        )
    };

    // Re-register the global shortcut if it changed
    let current_shortcut = app.state::<AppState>().config_store.get().shortcut;
    if current_shortcut != config.shortcut {
        let gs = app.global_shortcut();
        let _ = gs.unregister_all();
        if let Some(shortcut) = new_shortcut {
            gs.register(shortcut)
                .map_err(|e| format!("Failed to register shortcut: {}", e))?;
        }
    }

    // Update autostart based on config
    let autostart = app.autolaunch();
    if config.auto_start {
        let _ = autostart.enable();
    } else {
        let _ = autostart.disable();
    }

    app.state::<AppState>().config_store.update(settings)
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    // If settings window already exists, focus it
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.set_focus();
        return;
    }

    let _ = WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("src/settings.html".into()))
        .title("Settings — English Input Assistant")
        .inner_size(400.0, 300.0)
        .min_inner_size(400.0, 300.0)
        .resizable(false)
        .build();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, _shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let app = app.clone();
                        thread::spawn(move || {
                            on_shortcut(app);
                        });
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            do_paste,
            restore_original_text,
            retry_translation,
            play_tts,
            stop_tts,
            preview_tts,
            get_gemini_models,
            get_tts_voices,
            get_config,
            save_config,
            open_settings,
            updater::check_for_updates,
        ])
        .setup(move |app| {
            // Initialize config store
            let app_data_dir = app.path().app_data_dir().expect("failed to get app data dir");
            let config_store = ConfigStore::new(app_data_dir);
            let shortcut_str = config_store.get().shortcut;
            app.manage(AppState {
                translation_id: AtomicU64::new(0),
                selected_text: Mutex::new(String::new()),
                #[cfg(target_os = "windows")]
                source_hwnd: Mutex::new(0),
                #[cfg(target_os = "macos")]
                source_pid: Mutex::new(0),
                audio_cache: AudioCache::new(),
                config_store,
                tts_stop: Mutex::new(None),
                tts_window: Mutex::new(None),
            });

            // Check once per process, independently of the hidden popup or Settings.
            let update_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = updater::check(update_app, None).await {
                    eprintln!("[updater] {error}");
                }
            });

            // System tray
            let settings_item = MenuItemBuilder::with_id("settings", "Settings").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .item(&settings_item)
                .separator()
                .item(&quit_item)
                .build()?;

            // macOS: use a dedicated template icon for the menu bar (black symbol,
            // transparent background). icon_as_template lets the OS adapt it to
            // dark/light mode automatically.
            #[cfg(target_os = "macos")]
            let tray_icon = {
                use image::GenericImageView;
                let path = app
                    .path()
                    .resource_dir()
                    .expect("resource dir")
                    .join("icons/tray-icon-mac.png");
                let img = image::open(&path).expect("failed to load tray icon");
                let (width, height) = img.dimensions();
                let rgba = img.into_rgba8().into_raw();
                tauri::image::Image::new_owned(rgba, width, height)
            };
            #[cfg(not(target_os = "macos"))]
            let tray_icon = app.default_window_icon().cloned()
                .expect("failed to load tray icon");

            let tray_builder = TrayIconBuilder::new()
                .icon(tray_icon)
                .menu(&tray_menu)
                .tooltip("English Input Assistant")
                .on_menu_event(|app: &AppHandle, event: tauri::menu::MenuEvent| match event.id().as_ref() {
                    "settings" => {
                        let _ = open_settings(app.clone());
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                });

            #[cfg(target_os = "macos")]
            let tray_builder = tray_builder.icon_as_template(true);

            tray_builder.build(app)?;

            if !shortcut_str.is_empty() {
                match Shortcut::from_str(&shortcut_str) {
                    Ok(shortcut) => {
                        app.global_shortcut().register(shortcut)?;
                        debug_log!("[rust] Global shortcut registered: {}", shortcut_str);
                    }
                    Err(_e) => {
                        debug_log!("[rust] Invalid shortcut in config: {} ({})", shortcut_str, _e);
                    }
                }
            } else {
                debug_log!("[rust] No shortcut configured");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
