use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{ipc::Channel, AppHandle};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

// Shared by the startup task and every Settings window, including reopened ones.
static UPDATE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

struct UpdateGuard<'a>(&'a AtomicBool);

impl<'a> UpdateGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(flag))
    }
}

impl Drop for UpdateGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateOutcome {
    UpToDate,
    Cancelled,
    Busy,
    Development,
}

#[derive(Clone, serde::Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum UpdateProgress {
    Downloading { received: u64, total: Option<u64> },
    Installing,
}

async fn confirm_install(app: AppHandle, version: String) -> Result<bool, String> {
    // Native blocking dialogs must run off the event loop.
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(format!(
                "English Input Assistant {version} が利用可能です。今すぐインストールして再起動しますか？\n\n続行する前に設定を保存し、実行中の翻訳を完了してください。"
            ))
            .title("アップデートがあります")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "インストールして再起動".into(),
                "後で".into(),
            ))
            .blocking_show()
    })
    .await
    .map_err(|e| e.to_string())
}

pub async fn check(
    app: AppHandle,
    on_progress: Option<Channel<UpdateProgress>>,
) -> Result<UpdateOutcome, String> {
    // Never replace a developer's running build with an installed release.
    if cfg!(debug_assertions) {
        return Ok(UpdateOutcome::Development);
    }
    let Some(_guard) = UpdateGuard::acquire(&UPDATE_IN_PROGRESS) else {
        return Ok(UpdateOutcome::Busy);
    };

    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("アップデートを初期化できませんでした: {e}"))?;
    let Some(update) = updater
        .check()
        .await
        .map_err(|e| format!("アップデートを確認できませんでした: {e}"))?
    else {
        return Ok(UpdateOutcome::UpToDate);
    };

    if !confirm_install(app.clone(), update.version.clone()).await? {
        return Ok(UpdateOutcome::Cancelled);
    }

    let mut received = 0;
    let result = update
        .download_and_install(
            |bytes, total| {
                received += bytes as u64;
                if let Some(channel) = &on_progress {
                    let _ = channel.send(UpdateProgress::Downloading { received, total });
                }
            },
            || {
                if let Some(channel) = &on_progress {
                    let _ = channel.send(UpdateProgress::Installing);
                }
            },
        )
        .await;
    if let Err(error) = result {
        let message = format!("アップデートをインストールできませんでした: {error}");
        // Once installation was accepted, show failures even for startup checks.
        app.dialog()
            .message(&message)
            .title("アップデート失敗")
            .kind(MessageDialogKind::Error)
            .show(|_| {});
        return Err(message);
    }

    // Windows NSIS restarts the app itself; on macOS restart after installation.
    app.restart();
}

#[tauri::command]
pub async fn check_for_updates(
    app: AppHandle,
    on_progress: Channel<UpdateProgress>,
) -> Result<UpdateOutcome, String> {
    check(app, Some(on_progress)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_checks_are_rejected_and_retry_is_allowed_after_completion() {
        let flag = AtomicBool::new(false);
        let guard = UpdateGuard::acquire(&flag).unwrap();
        assert!(UpdateGuard::acquire(&flag).is_none());
        drop(guard);
        assert!(UpdateGuard::acquire(&flag).is_some());
    }

    #[test]
    fn guard_is_released_when_a_task_unwinds() {
        let flag = AtomicBool::new(false);
        let _ = std::panic::catch_unwind(|| {
            let _guard = UpdateGuard::acquire(&flag).unwrap();
            panic!("simulated update failure");
        });
        assert!(UpdateGuard::acquire(&flag).is_some());
    }
}
