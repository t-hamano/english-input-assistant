//! Cross-platform keyboard and mouse input via enigo.

use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Mouse, Settings,
};

/// Cut/paste modifier: Cmd on macOS, Ctrl elsewhere.
#[cfg(target_os = "macos")]
const ACTION_MODIFIER: Key = Key::Meta;
#[cfg(not(target_os = "macos"))]
const ACTION_MODIFIER: Key = Key::Control;

fn new_enigo() -> Option<Enigo> {
    Enigo::new(&Settings::default()).ok()
}

fn send_action(key: char) -> Result<(), String> {
    let mut enigo = new_enigo().ok_or("キーボード操作を開始できませんでした。")?;
    let result = enigo.key(ACTION_MODIFIER, Press)
        .and_then(|_| enigo.key(Key::Unicode(key), Click));
    // Release even if pressing the shortcut failed.
    let release = enigo.key(ACTION_MODIFIER, Release);
    result.and(release).map_err(|_| "キーボード操作に失敗しました。".to_string())
}

pub fn send_cut() -> Result<(), String> {
    send_action('x')
}

pub fn send_paste() -> Result<(), String> {
    send_action('v')
}

pub fn release_modifiers() {
    if let Some(mut enigo) = new_enigo() {
        for key in [Key::Control, Key::Alt, Key::Shift, Key::Meta] {
            let _ = enigo.key(key, Release);
        }
    }
}

pub fn get_cursor_position() -> (i32, i32) {
    new_enigo()
        .and_then(|e| e.location().ok())
        .unwrap_or((0, 0))
}
