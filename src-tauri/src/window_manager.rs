//! Platform-specific window focus management.

/// Windows-specific window focus management via winapi.
#[cfg(target_os = "windows")]
mod imp {
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::{GetForegroundWindow, SetForegroundWindow};

    pub fn get_foreground_window() -> HWND {
        unsafe { GetForegroundWindow() }
    }

    pub fn set_foreground_window(hwnd: HWND) {
        unsafe {
            SetForegroundWindow(hwnd);
        }
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
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    /// Brings the application with the given PID to the foreground.
    pub fn set_foreground_pid(pid: i32) {
        if pid == 0 {
            return;
        }
        let script = format!(
            "tell application \"System Events\" to set frontmost of (first application process whose unix id is {}) to true",
            pid
        );
        let _ = Command::new("osascript").args(["-e", &script]).output();
    }

    /// Returns the PID of the current process.
    pub fn own_pid() -> i32 {
        std::process::id() as i32
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use imp::*;
