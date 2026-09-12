mod config;
mod google_api;
mod keyboard;
mod llm;
mod selection;
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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::{thread, time::Duration};

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use config::{ConfigStore, SettingsConfig};
use llm::TranslationResult;
use selection::{focus_matches, PendingSelection, Selection};
use tts::{AudioCache, StopSignal};

/// State shared between shortcut handler and commands.
struct AppState {
    translation_id: AtomicU64,
    // Also serializes clipboard/focus operations. Never held during an API call.
    selection: Mutex<PendingSelection>,
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
    let is_current = || {
        app.state::<AppState>()
            .translation_id
            .load(Ordering::SeqCst)
            == request_id
    };

    for attempt in 1..=max_retries {
        if !is_current() {
            return Err("翻訳をキャンセルしました".to_string());
        }
        match llm::translate(
            api_key,
            model,
            input,
            additional_prompt,
            |result| {
                displayed = true;
                emit_translation(app, request_id, &result, false);
            },
            is_current,
        ) {
            Ok(result) => return Ok(result),
            Err(e) => {
                debug_log!(
                    "[rust] LLM attempt {}/{} failed: {}",
                    attempt,
                    max_retries,
                    e
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
    if app
        .state::<AppState>()
        .translation_id
        .load(Ordering::SeqCst)
        == request_id
    {
        let _ = app.emit(
            "translation-update",
            serde_json::json!({
                "request_id": request_id, "result": result, "complete": complete,
            }),
        );
    }
}

fn emit_translation_error(app: &AppHandle, request_id: u64, message: impl Into<String>) {
    if app
        .state::<AppState>()
        .translation_id
        .load(Ordering::SeqCst)
        == request_id
    {
        let _ = app.emit(
            "translation-error",
            serde_json::json!({
                "request_id": request_id, "message": message.into(),
            }),
        );
    }
}

fn run_translation(app: &AppHandle, request_id: u64, input: &str) {
    let (api_key, model, additional_prompt) = match get_llm_config(app) {
        Ok(config) => config,
        Err(message) => {
            emit_translation_error(app, request_id, message);
            return;
        }
    };
    if api_key.is_empty() {
        emit_translation_error(
            app,
            request_id,
            "API キーが設定されていません。設定画面で Google API キーを登録してください。",
        );
        return;
    }
    match translate_with_retry(app, request_id, input, &api_key, &model, &additional_prompt) {
        Ok(result) => emit_translation(app, request_id, &result, true),
        Err(message) => emit_translation_error(app, request_id, message),
    }
}

fn get_llm_config(app: &AppHandle) -> Result<(String, String, String), String> {
    let config = app.state::<AppState>().config_store.get();
    Ok((
        app.state::<AppState>().config_store.api_key()?,
        config.gemini_model,
        config.additional_prompt,
    ))
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

/// Re-focus the captured source window and confirm the OS honored it. `false`
/// means the caller must not synthesize Ctrl+X / Ctrl+V — they would hit an
/// unrelated window.
#[cfg(target_os = "windows")]
fn restore_source_focus(source_hwnd: usize) -> bool {
    if source_hwnd == 0 {
        return false;
    }
    for attempt in 0..3 {
        if !window_manager::is_window(source_hwnd) {
            return false;
        }
        window_manager::set_foreground_window(source_hwnd);
        thread::sleep(Duration::from_millis(if attempt == 0 { 150 } else { 100 }));
        if focus_matches(source_hwnd, window_manager::get_foreground_window()) {
            return true;
        }
    }
    false
}

#[cfg(target_os = "macos")]
fn restore_source_focus(source: usize) -> bool {
    let Ok(source_pid) = i32::try_from(source) else {
        return false;
    };
    if !window_manager::set_foreground_pid(source_pid) {
        return false;
    }
    thread::sleep(Duration::from_millis(150));
    source_is_focused(source)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn restore_source_focus(_source: usize) -> bool {
    false
}

fn foreground_source() -> usize {
    #[cfg(target_os = "windows")]
    {
        window_manager::get_foreground_window()
    }
    #[cfg(target_os = "macos")]
    {
        window_manager::get_foreground_pid().max(0) as usize
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        0
    }
}

fn source_is_focused(source: usize) -> bool {
    focus_matches(source, foreground_source())
}

fn capture_source(_app: &AppHandle) -> Option<usize> {
    let source = foreground_source();
    if source == 0 {
        return None;
    }
    #[cfg(target_os = "windows")]
    for label in ["main", "settings"] {
        if _app
            .get_webview_window(label)
            .and_then(|w| w.hwnd().ok())
            .is_some_and(|hwnd| hwnd.0 as usize == source)
        {
            return None;
        }
    }
    #[cfg(target_os = "macos")]
    if source == window_manager::own_pid() as usize {
        return None;
    }
    Some(source)
}

fn hide_popup(app: &AppHandle) -> Result<(), String> {
    // Notify on every backend hide, including captures that later fail or are empty.
    app.emit_to("main", "popup-hiding", ())
        .map_err(|e| e.to_string())?;
    if let Some(win) = app.get_webview_window("main") {
        win.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|_| "クリップボードにアクセスできませんでした。".to_string())?;
    clipboard
        .set_text(text)
        .map_err(|_| "クリップボードを書き換えられませんでした。".to_string())
}

fn read_clipboard() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|_| "クリップボードにアクセスできませんでした。".to_string())?;
    clipboard
        .get_text()
        .map_err(|_| "クリップボードのテキストを読み取れませんでした。".to_string())
}

fn on_shortcut(app: AppHandle) {
    let state = app.state::<AppState>();
    // Ignore repeats while a cut/paste is in progress. A second operation must
    // never change the source window underneath the first one.
    let Ok(mut pending) = state.selection.try_lock() else {
        return;
    };
    let (cx, cy) = keyboard::get_cursor_position();
    if pending.current().is_some() {
        // Preserve the outstanding cut until the user replaces or restores it.
        show_popup(&app, cx, cy);
        return;
    }
    let request_id = state.translation_id.fetch_add(1, Ordering::SeqCst) + 1;
    let source = capture_source(&app);
    if hide_popup(&app).is_err() {
        return;
    }

    let captured = (|| {
        let source = source.ok_or("元のウィンドウを確認できませんでした。")?;
        thread::sleep(Duration::from_millis(300));
        keyboard::release_modifiers();
        thread::sleep(Duration::from_millis(50));
        if !restore_source_focus(source) {
            return Err("元のウィンドウにフォーカスを戻せませんでした。".to_string());
        }
        let text = selection::capture(
            || {
                write_clipboard("")?;
                // Confirm the clear succeeded before allowing a cut or later read.
                if !read_clipboard()?.is_empty() {
                    return Err("クリップボードを初期化できませんでした。".to_string());
                }
                Ok(())
            },
            || {
                if !source_is_focused(source) {
                    return Err("元のウィンドウのフォーカスが変わりました。".to_string());
                }
                keyboard::send_cut()
            },
            || {
                thread::sleep(Duration::from_millis(200));
                read_clipboard()
            },
        )?;
        Ok((source, text))
    })();
    let (source, text) = match captured {
        Ok(captured) => captured,
        Err(message) => {
            let _ = app.emit("show-error", message);
            show_popup(&app, cx, cy);
            return;
        }
    };
    if text.is_empty() {
        return;
    }
    const MAX_TEXT_LENGTH: usize = 5000;
    if text.trim().is_empty() || text.len() > MAX_TEXT_LENGTH {
        // Restore only to this capture's source, never a previously stored target.
        let message = match paste_text(&text, source) {
            Ok(()) => "テキストが空白のみ、または長すぎます（最大 5000 バイト）。短いテキストを選択してください。".to_string(),
            Err(error) => format!("{error} 原文はクリップボードから手動で復元してください。"),
        };
        let _ = app.emit("show-error", message);
        show_popup(&app, cx, cy);
        return;
    }
    pending.begin(Selection {
        request_id,
        text: text.clone(),
        source,
    });
    let _ = app.emit(
        "show-loading",
        serde_json::json!({
            "request_id": request_id, "text": text,
        }),
    );
    show_popup(&app, cx, cy);
    drop(pending);
    run_translation(&app, request_id, &text);
}

#[tauri::command]
fn restore_original_text(request_id: u64, app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut pending = state
        .selection
        .try_lock()
        .map_err(|_| "テキストを処理中です。もう一度お試しください。".to_string())?;
    // Consume before restoring: duplicate or delayed commands cannot paste again.
    let Some(selection) = pending.take(request_id) else {
        return Ok(());
    };
    state.translation_id.fetch_add(1, Ordering::SeqCst);
    // The frontend has already stopped recording before invoking this command.
    let _ = hide_popup(&app);
    if let Err(error) = paste_text(&selection.text, selection.source) {
        let message = match write_clipboard(&selection.text) {
            Ok(()) => format!("{error} 原文をクリップボードにコピーしました。貼り付け先を確認して手動で復元してください。"),
            Err(_) => {
                // Retain the original if neither pasting nor clipboard recovery worked.
                pending.begin(selection);
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
                return Err("原文を復元できませんでした。もう一度お試しください。".into());
            }
        };
        // No active selection remains, so closing this error cannot paste again.
        let _ = app.emit("show-error", message);
        let (x, y) = keyboard::get_cursor_position();
        show_popup(&app, x, y);
        return Err("原文はクリップボードから手動で復元してください。".into());
    }
    Ok(())
}

#[tauri::command]
fn do_paste(text: String, request_id: u64, app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut pending = state
        .selection
        .try_lock()
        .map_err(|_| "テキストを処理中です。もう一度お試しください。".to_string())?;
    let selection = pending
        .get(request_id)
        .ok_or("この翻訳はすでに終了しています。")?;
    hide_popup(&app)?;
    if let Err(error) = paste_text(&text, selection.source) {
        // Keep the original bound to its source so Close can still restore it.
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.show();
            let _ = win.set_focus();
        }
        return Err(error);
    }
    pending.take(request_id);
    state.translation_id.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

/// Recheck focus immediately before synthesizing paste, including rollback paths.
fn paste_text(text: &str, source: usize) -> Result<(), String> {
    write_clipboard(text)?;
    if !restore_source_focus(source) {
        return Err("元のウィンドウにフォーカスを戻せませんでした。".into());
    }
    keyboard::release_modifiers();
    thread::sleep(Duration::from_millis(50));
    if !source_is_focused(source) {
        return Err("元のウィンドウのフォーカスが変わりました。".into());
    }
    keyboard::send_paste()
}

#[tauri::command]
fn retry_translation(request_id: u64, app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut pending = state
        .selection
        .try_lock()
        .map_err(|_| "テキストを処理中です。もう一度お試しください。".to_string())?;
    pending
        .get(request_id)
        .ok_or("再試行できる翻訳がありません。")?;
    let next_id = state.translation_id.fetch_add(1, Ordering::SeqCst) + 1;
    let selection = pending.retry(request_id, next_id).unwrap();
    let _ = app.emit(
        "show-loading",
        serde_json::json!({
            "request_id": next_id, "text": selection.text,
        }),
    );
    drop(pending);
    thread::spawn(move || {
        run_translation(&app, next_id, &selection.text);
    });
    Ok(())
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
        .await
        .map_err(|_| "音声の再生を開始できませんでした。".to_string())
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
        .await
        .map_err(|_| "音声の再生を開始できませんでした。".to_string())
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

    let text = "The quick brown fox jumps over the lazy dog. A gentle breeze drifted through the open window as the afternoon light slowly faded away.".to_string();
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
        .await
        .map_err(|_| "設定を読み込めませんでした。".to_string())?
}

#[tauri::command]
async fn save_config(config: SettingsConfig, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || save_settings(config, app))
        .await
        .map_err(|_| "設定を保存できませんでした。".to_string())?
}

fn save_settings(settings: SettingsConfig, app: AppHandle) -> Result<(), String> {
    let config = &settings.preferences;
    // Validate the new shortcut before doing anything else (empty = no shortcut)
    let new_shortcut = if config.shortcut.is_empty() {
        None
    } else {
        Some(
            Shortcut::from_str(&config.shortcut)
                .map_err(|e| format!("無効なショートカット \"{}\": {}", config.shortcut, e))?,
        )
    };

    // Re-register the global shortcut if it changed
    let current_shortcut = app.state::<AppState>().config_store.get().shortcut;
    if current_shortcut != config.shortcut {
        let gs = app.global_shortcut();
        let _ = gs.unregister_all();
        if let Some(shortcut) = new_shortcut {
            gs.register(shortcut)
                .map_err(|e| format!("ショートカットの登録に失敗しました: {}", e))?;
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

    let _ = WebviewWindowBuilder::new(
        &app,
        "settings",
        WebviewUrl::App("src/settings.html".into()),
    )
    .title("設定 — English Input Assistant")
    .inner_size(500.0, 300.0)
    .min_inner_size(500.0, 300.0)
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
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");
            let config_store = ConfigStore::new(app_data_dir);
            let shortcut_str = config_store.get().shortcut;
            let auto_check_updates = config_store.get().auto_check_updates;
            app.manage(AppState {
                translation_id: AtomicU64::new(0),
                selection: Mutex::new(PendingSelection::default()),
                audio_cache: AudioCache::new(),
                config_store,
                tts_stop: Mutex::new(None),
                tts_window: Mutex::new(None),
            });

            // Check once per process, independently of the hidden popup or Settings.
            if auto_check_updates {
                let update_app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = updater::check(update_app, None).await {
                        eprintln!("[updater] {error}");
                    }
                });
            }

            // System tray
            let settings_item = MenuItemBuilder::with_id("settings", "設定").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "終了").build(app)?;
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
            let tray_icon = app
                .default_window_icon()
                .cloned()
                .expect("failed to load tray icon");

            let tray_builder = TrayIconBuilder::new()
                .icon(tray_icon)
                .menu(&tray_menu)
                .tooltip("English Input Assistant")
                .on_menu_event(|app: &AppHandle, event: tauri::menu::MenuEvent| {
                    match event.id().as_ref() {
                        "settings" => {
                            open_settings(app.clone());
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
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
                        debug_log!(
                            "[rust] Invalid shortcut in config: {} ({})",
                            shortcut_str,
                            _e
                        );
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
