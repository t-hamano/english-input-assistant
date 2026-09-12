//! Platform-specific window focus management.

/// Windows-specific window focus management via winapi.
#[cfg(target_os = "windows")]
mod imp {
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::{GetForegroundWindow, IsWindow, SetForegroundWindow};

    /// Foreground window handle as a raw `usize` (`0` = none).
    pub fn get_foreground_window() -> usize {
        unsafe { GetForegroundWindow() as usize }
    }

    /// Request activation of `hwnd`; returns whether the OS accepted it.
    pub fn set_foreground_window(hwnd: usize) -> bool {
        unsafe { SetForegroundWindow(hwnd as HWND) != 0 }
    }

    /// Whether `hwnd` still refers to an existing window.
    pub fn is_window(hwnd: usize) -> bool {
        hwnd != 0 && unsafe { IsWindow(hwnd as HWND) != 0 }
    }
}

/// macOS window focus management via osascript.
/// TODO: replace with native NSWorkspace / NSRunningApplication calls for lower latency.
#[cfg(target_os = "macos")]
mod imp {
    use std::process::Command;

    /// Returns the PID of the currently frontmost application.
    pub fn get_foreground_pid() -> i32 {
        let output = Command::new("osascript")
            .args([
                "-e",
                "tell application \"System Events\" to unix id of (first application process whose frontmost is true)",
            ])
            .output()
            .ok();
        output
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<i32>().ok())
            .filter(|pid| *pid > 0)
            .unwrap_or(0)
    }

    /// Brings the application with the given PID to the foreground.
    pub fn set_foreground_pid(pid: i32) -> bool {
        if pid <= 0 {
            return false;
        }
        let script = format!(
            "tell application \"System Events\" to set frontmost of (first application process whose unix id is {}) to true",
            pid
        );
        Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Returns the PID of the current process.
    pub fn own_pid() -> i32 {
        std::process::id() as i32
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use imp::*;
