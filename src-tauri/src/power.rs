use crate::Shell;
use download_engine::{DownloadId, DownloadState};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, atomic::Ordering};
use tauri::Manager;
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CompletionAction {
    Shutdown,
    Disconnect { connection: String },
}

#[derive(Clone, Serialize)]
pub(crate) struct PowerPlan {
    pub queue_id: DownloadId,
    pub action: CompletionAction,
    pub deadline: Option<u64>,
    #[serde(skip)]
    timer: Option<tokio::time::Instant>,
    #[serde(skip)]
    notice_seen: Option<tokio::time::Instant>,
}
pub(crate) type PowerState = Arc<Mutex<Option<PowerPlan>>>;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn countdown_ready(plan: &PowerPlan, now: tokio::time::Instant, visible: bool) -> bool {
    visible
        && plan
            .timer
            .is_some_and(|timer| now.duration_since(timer).as_secs() >= 60)
        && plan
            .notice_seen
            .is_some_and(|seen| now.duration_since(seen).as_secs() <= 3)
}

#[tauri::command]
pub(crate) async fn power_status(
    state: tauri::State<'_, PowerState>,
) -> Result<Option<PowerPlan>, String> {
    Ok(state.lock().await.clone().map(|mut plan| {
        if let Some(timer) = plan.timer {
            plan.deadline = Some(now() + 60u64.saturating_sub(timer.elapsed().as_secs()));
        }
        plan
    }))
}

#[tauri::command]
pub(crate) async fn power_cancel(state: tauri::State<'_, PowerState>) -> Result<(), String> {
    *state.lock().await = None;
    Ok(())
}

#[tauri::command]
pub(crate) async fn power_acknowledge<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, PowerState>,
) -> Result<(), String> {
    let visible = app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true)
    });
    if visible
        && let Some(plan) = state.lock().await.as_mut()
        && plan.deadline.is_some()
    {
        plan.notice_seen = Some(tokio::time::Instant::now());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn power_arm(
    shell: tauri::State<'_, Shell>,
    state: tauri::State<'_, PowerState>,
    queue_id: DownloadId,
    action: CompletionAction,
    consent: bool,
) -> Result<(), String> {
    if osdm_browser_bridge::native_test_directory()
        .map_err(|_| "Invalid native test directory")?
        .is_some()
    {
        return Err("Power actions are disabled during native browser tests".into());
    }
    if !cfg!(windows) {
        return Err("Completion actions currently require Windows".into());
    }
    if !consent {
        return Err("Explicit consent is required".into());
    }
    if let CompletionAction::Disconnect { connection } = &action
        && (connection.trim().is_empty()
            || connection.starts_with('/')
            || connection.starts_with('-')
            || connection.len() > 256
            || connection.chars().any(char::is_control))
    {
        return Err("Choose an existing Windows RAS/VPN connection name".into());
    }
    let engine = shell.engine().await?;
    let automation = engine.automation().await;
    if !automation.queues.iter().any(|q| q.id == queue_id)
        || !automation.membership.values().any(|q| *q == queue_id)
    {
        return Err("Choose a nonempty named queue".into());
    }
    *state.lock().await = Some(PowerPlan {
        queue_id,
        action,
        deadline: None,
        timer: None,
        notice_seen: None,
    });
    Ok(())
}

pub(crate) fn start(app: &tauri::AppHandle) {
    let state: PowerState = Arc::new(Mutex::new(None));
    app.manage(state.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let mut plan = state.lock().await;
            let Some(current) = plan.as_mut() else {
                continue;
            };
            let shell = app.state::<Shell>();
            let Ok(engine) = shell.engine().await else {
                *plan = None;
                continue;
            };
            let automation = engine.automation().await;
            let downloads = engine.snapshot().await;
            let selected: Vec<_> = downloads
                .iter()
                .filter(|d| automation.membership.get(&d.id) == Some(&current.queue_id))
                .collect();
            if selected.is_empty()
                || selected
                    .iter()
                    .any(|d| matches!(d.state, DownloadState::Failed | DownloadState::Canceled))
            {
                *plan = None;
                continue;
            }
            let complete = selected.iter().all(|d| d.state == DownloadState::Completed);
            // Any new user work cancels an already-visible countdown. It never
            // silently pauses unrelated work just to execute a power action.
            let active = !automation.pending_captures.is_empty()
                || automation.queues.iter().any(|q| q.checking_sources)
                || downloads.iter().any(|d| {
                    matches!(
                        d.state,
                        DownloadState::Queued
                            | DownloadState::Downloading
                            | DownloadState::Pausing
                            | DownloadState::Canceling
                    )
                });
            if current.deadline.is_some() && (!complete || active) {
                *plan = None;
                crate::report(
                    &app,
                    "Completion action canceled because download work changed".into(),
                );
                continue;
            }
            if !complete || active {
                continue;
            }
            if current.deadline.is_none() {
                current.deadline = Some(now() + 60);
                current.timer = Some(tokio::time::Instant::now());
                crate::show_window(&app);
            }
            if current
                .timer
                .is_none_or(|timer| timer.elapsed().as_secs() < 60)
            {
                continue;
            }
            if !countdown_ready(
                current,
                tokio::time::Instant::now(),
                app.get_webview_window("main").is_some_and(|window| {
                    window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true)
                }),
            ) {
                *plan = None;
                crate::report(
                    &app,
                    "Completion action canceled because its countdown was not visibly acknowledged"
                        .into(),
                );
                continue;
            }
            let action = current.action.clone();
            *plan = None;
            // Keep cancellation serialized until the final decision, then prevent
            // new shell work and atomically quiesce the engine only if still idle.
            shell.quitting.store(true, Ordering::Release);
            drop(plan);
            if !matches!(engine.shutdown_if_idle().await, Ok(true)) {
                shell.quitting.store(false, Ordering::Release);
                crate::report(
                    &app,
                    "Completion action canceled: engine is busy or could not save".into(),
                );
                continue;
            }
            let _ = shell.shutdown().await;
            let result = execute_action(&action).await;
            shell.quitting.store(false, Ordering::Release);
            let _ = crate::engine_open(
                app.clone(),
                app.state::<Shell>(),
                download_engine::Config::default(),
            )
            .await;
            if result.is_err() {
                crate::report(
                    &app,
                    "Windows refused the completion action; downloads remain saved".into(),
                );
            }
        }
    });
}

async fn execute_action(action: &CompletionAction) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let system = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| std::io::Error::other("missing Windows directory"))?
            .join("System32");
        let mut command = match action {
            CompletionAction::Shutdown => {
                let mut c = tokio::process::Command::new(system.join("shutdown.exe"));
                c.args(["/s", "/t", "0"]);
                c
            }
            CompletionAction::Disconnect { connection } => {
                let mut c = tokio::process::Command::new(system.join("rasdial.exe"));
                c.arg(connection).arg("/disconnect");
                c
            }
        };
        command
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let status = tokio::time::timeout(std::time::Duration::from_secs(15), command.status())
            .await
            .map_err(|_| std::io::Error::other("completion action timeout"))??;
        if !status.success() {
            return Err(std::io::Error::other("completion action failed"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = action;
        Err(std::io::Error::other("unsupported platform"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn action_requires_elapsed_countdown_and_fresh_visible_ui_acknowledgement() {
        let now = tokio::time::Instant::now();
        let mut plan = PowerPlan {
            queue_id: DownloadId::new_v4(),
            action: CompletionAction::Shutdown,
            deadline: Some(1),
            timer: Some(now - std::time::Duration::from_secs(59)),
            notice_seen: Some(now),
        };
        assert!(!countdown_ready(&plan, now, true));
        plan.timer = Some(now - std::time::Duration::from_secs(60));
        assert!(countdown_ready(&plan, now, true));
        assert!(!countdown_ready(&plan, now, false));
        plan.notice_seen = None;
        assert!(!countdown_ready(&plan, now, true));
        plan.notice_seen = Some(now - std::time::Duration::from_secs(4));
        assert!(!countdown_ready(&plan, now, true));
        // No operating-system action is invoked by these policy tests.
    }
}
