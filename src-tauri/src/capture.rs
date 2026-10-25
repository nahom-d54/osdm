use crate::Shell;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use download_engine::{CapturedHeaders, DownloadId, DownloadRequest};
use osdm_browser_bridge::{Direction, Envelope, open, seal};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::Manager;

pub(crate) struct CaptureBridge {
    app: tauri::AppHandle,
    token: String,
    pub enabled: AtomicBool,
    config: PathBuf,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Message {
    Ping,
    Status {
        id: DownloadId,
    },
    Queues,
    StartQueue {
        id: DownloadId,
    },
    StopQueue {
        id: DownloadId,
    },
    Prepare {
        url: String,
        filename: Option<String>,
        headers: Option<CapturedHeaders>,
        explicit_credentials: Option<bool>,
    },
    Commit {
        id: DownloadId,
    },
    Abort {
        id: DownloadId,
    },
}

fn safe_filename(value: &str) -> Option<String> {
    let stem = value.split('.').next()?.to_ascii_uppercase();
    if value.is_empty()
        || value.len() > 240
        || value.ends_with(['.', ' '])
        || value
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        None
    } else {
        Some(value.into())
    }
}

pub(crate) fn start(app: &tauri::AppHandle, directory: PathBuf) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result: Result<(), Box<dyn std::error::Error + Send + Sync>> = async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let endpoint = osdm_browser_bridge::Endpoint::new(listener.local_addr()?.port());
            let config = directory.join("capture-enabled");
            let enabled = std::fs::read_to_string(&config).is_ok_and(|s| s == "enabled");
            endpoint.save(&directory.join("capture-bridge.bin"))?;
            let state = Arc::new(CaptureBridge {
                app: app.clone(),
                token: endpoint.token,
                enabled: AtomicBool::new(enabled),
                config,
            });
            app.manage(state.clone());
            let router = Router::new()
                .route("/v1/capture", post(handle))
                .layer(DefaultBodyLimit::max(400 * 1024))
                .with_state(state);
            axum::serve(listener, router).await?;
            Ok(())
        }
        .await;
        if result.is_err() {
            crate::report(
                &app,
                "Browser bridge could not start; browser downloads remain available".into(),
            );
        }
    });
}

#[tauri::command]
pub(crate) fn capture_status(state: tauri::State<'_, Arc<CaptureBridge>>) -> bool {
    state.enabled.load(Ordering::Acquire)
}

#[tauri::command]
pub(crate) fn capture_enable(
    state: tauri::State<'_, Arc<CaptureBridge>>,
    enabled: bool,
) -> Result<(), String> {
    std::fs::write(&state.config, if enabled { "enabled" } else { "disabled" })
        .map_err(|_| "Could not save capture preference".to_string())?;
    state.enabled.store(enabled, Ordering::Release);
    Ok(())
}

async fn handle(
    State(state): State<Arc<CaptureBridge>>,
    headers: HeaderMap,
    Json(envelope): Json<Envelope>,
) -> Result<Json<Envelope>, StatusCode> {
    let message =
        authenticated_message(&headers, &envelope, &state.token).ok_or(StatusCode::UNAUTHORIZED)?;
    let Json(reply) = handle_authorized(state.clone(), message).await?;
    Ok(Json(
        seal(&state.token, &reply, Direction::Response)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    ))
}

fn authenticated_message(headers: &HeaderMap, envelope: &Envelope, token: &str) -> Option<Message> {
    if headers.contains_key("origin") {
        return None;
    }
    serde_json::from_value(open(token, envelope, Direction::Request).ok()?).ok()
}

async fn handle_authorized(
    state: Arc<CaptureBridge>,
    message: Message,
) -> Result<Json<Value>, StatusCode> {
    if !state.enabled.load(Ordering::Acquire) {
        return Ok(Json(json!({"ok":false,"error":"disabled"})));
    }
    let shell = state.app.state::<Shell>();
    let result: Result<Value, &'static str> = async {
        let engine = shell.engine().await.map_err(|_| "not_ready")?;
        match message {
            Message::Ping => Ok(json!({"ok":true,"version":1})),
            Message::Status { id } => {
                let download = engine.snapshot().await.into_iter().find(|d| d.id == id).ok_or("not_found")?;
                let pending = engine.automation().await.pending_captures.contains_key(&id);
                Ok(json!({"ok":true,"state":download.state,"pending":pending,"sha256":download.sha256,"integrity":download.integrity}))
            }
            Message::Queues => Ok(json!({"ok":true,"state":engine.automation().await})),
            Message::StartQueue { id } => {
                let state = engine
                    .configure_automation(download_engine::AutomationCommand::Start { id })
                    .await
                    .map_err(|_| "queue_start_failed")?;
                Ok(json!({"ok":true,"state":state}))
            }
            Message::StopQueue { id } => {
                let state = engine
                    .configure_automation(download_engine::AutomationCommand::Stop { id })
                    .await
                    .map_err(|_| "queue_stop_failed")?;
                Ok(json!({"ok":true,"state":state}))
            }
            Message::Prepare {
                url,
                filename,
                headers,
                explicit_credentials,
            } => {
                if url.len() > 32768 || (headers.is_some() && explicit_credentials != Some(true)) {
                    return Err("invalid_request");
                }
                let mut url = reqwest::Url::parse(&url).map_err(|_| "invalid_url")?;
                if !matches!(url.scheme(), "http" | "https")
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err("invalid_url");
                }
                url.set_fragment(None);
                let suggested =
                    filename.unwrap_or_else(|| url.path().rsplit('/').next().unwrap_or("").into());
                let filename = safe_filename(&suggested).ok_or("unsafe_filename")?;
                let settings = shell
                    .settings
                    .load()
                    .await
                    .map_err(|_| "settings_failed")?
                    .unwrap_or_default();
                let directory = if settings.default_directory.is_empty() {
                    PathBuf::from(
                        crate::default_download_directory(state.app.clone())
                            .await
                            .map_err(|_| "destination_required")?,
                    )
                } else {
                    PathBuf::from(settings.default_directory)
                };
                let parent = tokio::fs::canonicalize(directory)
                    .await
                    .map_err(|_| "destination_required")?;
                let destination = parent.join(filename);
                let automation = engine.automation().await;
                if let Some(old) = engine
                    .snapshot()
                    .await
                    .into_iter()
                    .find(|d| d.url == url.as_str() && d.destination == destination)
                {
                    if automation.pending_captures.contains_key(&old.id) {
                        return Err("handoff_busy");
                    }
                    return Ok(json!({"ok":true,"id":old.id,"duplicate":true}));
                }
                let id = engine
                    .prepare_capture(DownloadRequest {
                        url: url.to_string(),
                        destination,
                        expected_sha256: None,
                    })
                    .await
                    .map_err(|_| "admission_failed")?;
                if let Some(headers) = headers
                    && engine.set_captured_headers(id, headers).await.is_err()
                {
                    let _ = engine.abort_capture(id).await;
                    return Err("invalid_credentials");
                }
                Ok(json!({"ok":true,"id":id,"duplicate":false}))
            }
            Message::Commit { id } => {
                engine
                    .commit_capture(id)
                    .await
                    .map_err(|_| "commit_failed")?;
                Ok(json!({"ok":true}))
            }
            Message::Abort { id } => {
                engine.abort_capture(id).await.map_err(|_| "abort_failed")?;
                Ok(json!({"ok":true}))
            }
        }
    }
    .await;
    Ok(Json(
        result.unwrap_or_else(|error| json!({"ok":false,"error":error})),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_web_origins_missing_or_wrong_tokens_and_unsafe_filenames() {
        let mut headers = HeaderMap::new();
        let endpoint = osdm_browser_bridge::Endpoint::new(12345);
        let message = seal(&endpoint.token, &json!({"op":"ping"}), Direction::Request).unwrap();
        assert!(authenticated_message(&headers, &message, &endpoint.token).is_some());
        assert!(
            authenticated_message(
                &headers,
                &message,
                &osdm_browser_bridge::Endpoint::new(12345).token
            )
            .is_none()
        );
        headers.insert("origin", "https://untrusted.invalid".parse().unwrap());
        assert!(authenticated_message(&headers, &message, &endpoint.token).is_none());
        for name in ["..", "CON.txt", "a/b.zip", "a\\b.zip", "stream:ads", ""] {
            assert!(safe_filename(name).is_none());
        }
        assert_eq!(
            safe_filename("ordinary.zip").as_deref(),
            Some("ordinary.zip")
        );
        assert!(
            serde_json::from_value::<Message>(
                json!({"op":"prepare","url":"https://example.invalid/a","unknown":"ignored?"})
            )
            .is_err()
        );
    }
}
