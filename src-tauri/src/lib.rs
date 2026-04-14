mod config;
mod llm;
mod tts;

macro_rules! debug_log {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        println!($($arg)*);
    };
}

use std::sync::Mutex;
use std::{thread, time::Duration};

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

use config::{AppConfig, ConfigStore};
use llm::TranslationResult;
use tts::{AudioCache, StopSignal};

/// Global shortcut key for triggering translation.
const SHORTCUT_MODIFIERS: Modifiers = Modifiers::CONTROL.union(Modifiers::ALT);
const SHORTCUT_KEY: Code = Code::Space;
#[cfg(debug_assertions)]
const SHORTCUT_LABEL: &str = "Ctrl+Alt+Space";

#[cfg(target_os = "windows")]
mod keyboard {
    use std::mem;
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::{
        GetCursorPos, GetForegroundWindow, SendInput, SetForegroundWindow, INPUT, INPUT_KEYBOARD,
        KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL,
    };

    fn make_key_input(vk: u16, flags: u32) -> INPUT {
        let mut input: INPUT = unsafe { mem::zeroed() };
        input.type_ = INPUT_KEYBOARD;
        unsafe {
            let ki = input.u.ki_mut();
            *ki = KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            };
        }
        input
    }

    pub fn send_key_combo(vk_modifier: u16, vk_key: u16) {
        let mut inputs = [
            make_key_input(vk_modifier, 0),
            make_key_input(vk_key, 0),
            make_key_input(vk_key, KEYEVENTF_KEYUP),
            make_key_input(vk_modifier, KEYEVENTF_KEYUP),
        ];
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_mut_ptr(),
                mem::size_of::<INPUT>() as i32,
            );
        }
    }

    pub fn release_modifiers() {
        let modifiers = [0x11u16, 0x12, 0x10, 0x5B];
        let mut inputs: Vec<INPUT> = modifiers
            .iter()
            .map(|&vk| make_key_input(vk, KEYEVENTF_KEYUP))
            .collect();
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_mut_ptr(),
                mem::size_of::<INPUT>() as i32,
            );
        }
    }

    pub fn get_cursor_position() -> (i32, i32) {
        let mut point = winapi::shared::windef::POINT { x: 0, y: 0 };
        unsafe {
            GetCursorPos(&mut point);
        }
        (point.x, point.y)
    }

    pub fn get_foreground_window() -> HWND {
        unsafe { GetForegroundWindow() }
    }

    pub fn set_foreground_window(hwnd: HWND) {
        unsafe {
            SetForegroundWindow(hwnd);
        }
    }

    pub const VK_X: u16 = 0x58;
    pub const VK_V: u16 = 0x56;
    pub const VK_CTRL: u16 = VK_CONTROL as u16;
}

/// State shared between shortcut handler and commands.
struct AppState {
    selected_text: Mutex<String>,
    #[allow(dead_code)]
    source_hwnd: Mutex<usize>,
    audio_cache: AudioCache,
    config_store: ConfigStore,
    tts_stop: Mutex<Option<StopSignal>>,
    tts_window: Mutex<Option<String>>,
}

/// Call LLM with automatic retry (up to 3 attempts).
fn translate_with_retry(
    input: &str,
    api_key: &str,
    model: &str,
    additional_prompt: &str,
) -> Result<TranslationResult, String> {
    let max_retries = 3;

    for attempt in 1..=max_retries {
        match llm::translate(api_key, model, input, additional_prompt) {
            Ok(result) => return Ok(result),
            Err(e) => {
                debug_log!(
                    "[rust] LLM attempt {}/{} failed: {}",
                    attempt, max_retries, e
                );
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

fn get_llm_config(app: &AppHandle) -> (String, String, String) {
    let config = app.state::<AppState>().config_store.get();
    (config.api_key().to_string(), config.gemini_model, config.additional_prompt)
}

#[cfg(target_os = "windows")]
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

            let fx = if x + popup_width > screen_right {
                x - popup_width
            } else {
                x
            };
            let fy = if y + popup_height > screen_bottom {
                y - popup_height
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
        let _ = win.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
            popup_width as u32,
            popup_height as u32,
        )));
        let _ = win.set_always_on_top(true);
        let _ = win.set_focus();
        let _ = win.show();
    }
}

#[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
fn on_shortcut(app: AppHandle) {
    debug_log!("[rust] shortcut triggered");

    #[cfg(target_os = "windows")]
    {
        use keyboard::*;

        let (cx, cy) = get_cursor_position();
        let fg_hwnd = get_foreground_window();

        // Collect HWNDs of all our own windows
        let own_hwnds: Vec<usize> = ["main", "settings"]
            .iter()
            .filter_map(|label| {
                app.get_webview_window(label)
                    .and_then(|w| w.hwnd().ok())
                    .map(|h| h.0 as usize)
            })
            .collect();

        let fg_is_own = own_hwnds.contains(&(fg_hwnd as usize));

        let source_hwnd = if fg_is_own {
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
        };

        // Hide popup if visible, then restore focus to source window
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.hide();
        }
        if source_hwnd != 0 {
            set_foreground_window(source_hwnd as *mut _);
        }
        thread::sleep(Duration::from_millis(150));

        // Wait for user to release shortcut keys
        thread::sleep(Duration::from_millis(300));
        release_modifiers();
        thread::sleep(Duration::from_millis(50));

        // Clear clipboard before Ctrl+X so we can detect if anything was actually selected
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text("");
        }
        thread::sleep(Duration::from_millis(50));

        // Simulate Ctrl+X to cut selected text
        send_key_combo(VK_CTRL, VK_X);
        debug_log!("[rust] Ctrl+X sent");
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
            send_key_combo(VK_CTRL, VK_V);
            let _ = app.emit("show-error", "Text is too long (max 5000 characters). Please select a shorter text.");
            show_popup(&app, cx, cy);
            return;
        }

        // Store selected text for retry
        if let Some(state) = app.try_state::<AppState>() {
            *state.selected_text.lock().unwrap() = selected_text.clone();
        }

        // Show popup with loading state
        let _ = app.emit("show-loading", &selected_text);
        show_popup(&app, cx, cy);

        // Call LLM API
        let (api_key, model, additional_prompt) = get_llm_config(&app);

        if api_key.is_empty() {
            let result = TranslationResult {
                translated: format!("[Translated] {}", selected_text),
                explanation: "N/A (no API key set)".to_string(),
                source_is_english: false,
            };
            let _ = app.emit("show-result", &result);
            return;
        }

        debug_log!("[rust] calling LLM...");
        match translate_with_retry(&selected_text, &api_key, &model, &additional_prompt) {
            Ok(result) => {
                debug_log!("[rust] LLM result: {:?}", result);
                let _ = app.emit("show-result", &result);
            }
            Err(e) => {
                debug_log!("[rust] LLM error: {}", e);
                let _ = app.emit("show-error", &e);
            }
        }
    }
}

#[tauri::command]
fn restore_original_text(app: AppHandle) {
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
#[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
fn do_paste(text: String, app: AppHandle) {
    #[cfg(target_os = "windows")]
    {
        use keyboard::*;

        // Hide popup and restore focus to source window
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.hide();
        }

        let source_hwnd = *app.state::<AppState>().source_hwnd.lock().unwrap();
        if source_hwnd != 0 {
            set_foreground_window(source_hwnd as *mut _);
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

        release_modifiers();
        thread::sleep(Duration::from_millis(50));
        send_key_combo(VK_CTRL, VK_V);
        debug_log!("[rust] pasted: {:?}", text);
    }
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

    let _ = app.emit("show-loading", &selected_text);

    thread::spawn(move || {
        let (api_key, model, additional_prompt) = get_llm_config(&app);
        match translate_with_retry(&selected_text, &api_key, &model, &additional_prompt) {
            Ok(result) => {
                let _ = app.emit("show-result", &result);
            }
            Err(e) => {
                let _ = app.emit("show-error", &e);
            }
        }
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
fn play_tts(text: String, app: AppHandle) {
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

    let api_key = config.api_key().to_string();

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
fn preview_tts(voice: String, speed: f64, app: AppHandle) {
    start_tts(&app, "settings");

    let api_key = app.state::<AppState>().config_store.get().api_key().to_string();
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
fn get_config(app: AppHandle) -> AppConfig {
    app.state::<AppState>().config_store.get()
}

#[tauri::command]
fn save_config(config: AppConfig, app: AppHandle) -> Result<(), String> {
    // Update autostart based on config
    let autostart = app.autolaunch();
    if config.auto_start {
        let _ = autostart.enable();
    } else {
        let _ = autostart.disable();
    }

    app.state::<AppState>().config_store.update(config)
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
        .resizable(false)
        .build();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let shortcut = Shortcut::new(Some(SHORTCUT_MODIFIERS), SHORTCUT_KEY);

    tauri::Builder::default()
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
        ])
        .setup(move |app| {
            // Initialize config store
            let app_data_dir = app.path().app_data_dir().expect("failed to get app data dir");
            let config_store = ConfigStore::new(app_data_dir);
            app.manage(AppState {
                selected_text: Mutex::new(String::new()),
                source_hwnd: Mutex::new(0),
                audio_cache: AudioCache::new(),
                config_store,
                tts_stop: Mutex::new(None),
                tts_window: Mutex::new(None),
            });

            // System tray
            let settings_item = MenuItemBuilder::with_id("settings", "Settings").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .item(&settings_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let tray_icon = app.default_window_icon().cloned()
                .expect("failed to load tray icon");

            TrayIconBuilder::new()
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
                })
                .build(app)?;

            app.global_shortcut().register(shortcut)?;
            debug_log!("[rust] Global shortcut registered: {}", SHORTCUT_LABEL);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
