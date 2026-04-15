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

pub fn send_cut() {
    if let Some(mut enigo) = new_enigo() {
        let _ = enigo.key(ACTION_MODIFIER, Press);
        let _ = enigo.key(Key::Unicode('x'), Click);
        let _ = enigo.key(ACTION_MODIFIER, Release);
    }
}

pub fn send_paste() {
    if let Some(mut enigo) = new_enigo() {
        let _ = enigo.key(ACTION_MODIFIER, Press);
        let _ = enigo.key(Key::Unicode('v'), Click);
        let _ = enigo.key(ACTION_MODIFIER, Release);
    }
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
