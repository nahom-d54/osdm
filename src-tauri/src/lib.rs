mod settings;
mod updates;

use download_engine::{
    Config, Download, DownloadId, DownloadRequest, DownloadState, Engine, EngineEvent, ProbeInfo,
};
use settings::{Settings, SettingsStore};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{
    AppHandle, Emitter, Manager, State,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

type Reply<T> = Result<T, String>;

struct Running {
    engine: Engine,
    events: tauri::async_runtime::JoinHandle<()>,
}

pub(crate) struct Shell {
    running: Mutex<Option<Running>>,
    settings: SettingsStore,
    database: PathBuf,
    quitting: AtomicBool,
    allow_exit: AtomicBool,
    updating: AtomicBool,
    tray_available: AtomicBool,
}

impl Shell {
    async fn engine(&self) -> Reply<Engine> {
        if self.quitting.load(Ordering::Acquire) {
            return Err("application is shutting down".into());
        }
        self.running
            .lock()
            .await
            .as_ref()
            .map(|r| r.engine.clone())
            .ok_or_else(|| "download engine is not open".into())
    }

    pub(crate) async fn shutdown(&self) -> Reply<()> {
        let mut running = self.running.lock().await;
        if let Some(active) = running.as_ref() {
            active.engine.shutdown().await.map_err(|e| e.to_string())?;
        }
        if let Some(active) = running.take() {
            active.events.abort();
            let _ = active.events.await;
        }
        Ok(())
    }
}

#[tauri::command]
async fn engine_open<R: tauri::Runtime>(
    app: AppHandle<R>,
    shell: State<'_, Shell>,
    config: Config,
) -> Reply<()> {
    let mut running = shell.running.lock().await;
    if shell.quitting.load(Ordering::Acquire) {
        return Err("application is shutting down".into());
    }
    // React StrictMode and a webview reload may request open again. The existing
    // engine remains authoritative; concurrency changes apply after app restart.
    if running.is_some() {
        return Ok(());
    }
    let config = shell
        .settings
        .load()
        .await?
        .map_or(config, |settings| settings.engine_config());
    let engine = Engine::open(&shell.database, config)
        .await
        .map_err(|e| e.to_string())?;
    let mut events = engine.subscribe();
    let initial = engine.snapshot().await;
    let mut completed: HashSet<_> = initial
        .iter()
        .filter(|d| d.state == DownloadState::Completed)
        .map(|d| d.id)
        .collect();
    let source = engine.clone();
    let events = tauri::async_runtime::spawn(async move {
        let mut previous = String::new();
        loop {
            let downloads = match events.recv().await {
                Ok(EngineEvent::Snapshot(downloads)) => downloads,
                Ok(EngineEvent::Error(error)) => {
                    let _ = app.emit("engine://error", error);
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => source.snapshot().await,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            for download in &downloads {
                if download.state == DownloadState::Completed && completed.insert(download.id) {
                    let name = download
                        .destination
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    if let Err(error) = app
                        .notification()
                        .builder()
                        .title("Download completed")
                        .body(name)
                        .show()
                    {
                        let _ = app.emit(
                            "engine://error",
                            format!("completion notification failed: {error}"),
                        );
                    }
                }
            }
            if let Ok(serialized) = serde_json::to_string(&downloads)
                && serialized != previous
            {
                previous = serialized;
                let _ = app.emit("engine://snapshot", downloads);
            }
        }
    });
    *running = Some(Running { engine, events });
    Ok(())
}

#[tauri::command]
async fn engine_shutdown(shell: State<'_, Shell>) -> Reply<()> {
    shell.shutdown().await
}

#[tauri::command]
async fn engine_enqueue(shell: State<'_, Shell>, request: DownloadRequest) -> Reply<DownloadId> {
    shell
        .engine()
        .await?
        .enqueue(request)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn engine_probe(shell: State<'_, Shell>, url: String) -> Reply<ProbeInfo> {
    shell
        .engine()
        .await?
        .probe(&url)
        .await
        .map_err(|e| e.to_string())
}

macro_rules! download_command {
    ($name:ident, $method:ident) => {
        #[tauri::command]
        async fn $name(shell: State<'_, Shell>, id: DownloadId) -> Reply<()> {
            shell
                .engine()
                .await?
                .$method(id)
                .await
                .map_err(|e| e.to_string())
        }
    };
}
download_command!(engine_pause, pause);
download_command!(engine_resume, resume);
download_command!(engine_retry, retry);
download_command!(engine_cancel, cancel);

#[tauri::command]
async fn engine_reorder(shell: State<'_, Shell>, ids: Vec<DownloadId>) -> Reply<()> {
    shell
        .engine()
        .await?
        .reorder(&ids)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
async fn engine_set_speed_limit(shell: State<'_, Shell>, bytes_per_second: u64) -> Reply<()> {
    shell
        .engine()
        .await?
        .set_speed_limit(bytes_per_second)
        .await;
    Ok(())
}

#[tauri::command]
async fn engine_snapshot(shell: State<'_, Shell>) -> Reply<Vec<Download>> {
    Ok(shell.engine().await?.snapshot().await)
}

#[tauri::command(rename_all = "camelCase")]
async fn engine_remove_download(
    shell: State<'_, Shell>,
    id: DownloadId,
    remove_partial_file: bool,
) -> Reply<()> {
    shell
        .engine()
        .await?
        .remove_download(id, remove_partial_file)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn engine_cleanup_partial_data(shell: State<'_, Shell>) -> Reply<usize> {
    shell
        .engine()
        .await?
        .cleanup_partial_data()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn settings_load(shell: State<'_, Shell>) -> Reply<Option<Settings>> {
    shell.settings.load().await
}

#[tauri::command]
async fn settings_save(shell: State<'_, Shell>, settings: Settings) -> Reply<()> {
    shell.settings.save(&settings).await
}

#[tauri::command(rename_all = "camelCase")]
async fn pick_directory(
    app: AppHandle,
    title: String,
    default_path: Option<String>,
) -> Reply<Option<String>> {
    let mut dialog = app.dialog().file().set_title(title);
    if let Some(path) = default_path.filter(|p| !p.is_empty()) {
        dialog = dialog.set_directory(path);
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    dialog.pick_folder(move |path| {
        let _ = send.send(path);
    });
    let selected = receive
        .await
        .map_err(|_| "directory dialog closed unexpectedly")?;
    selected
        .map(|path| {
            path.into_path()
                .map(|p| p.to_string_lossy().into_owned())
                .map_err(|e| e.to_string())
        })
        .transpose()
}

#[tauri::command]
async fn default_download_directory<R: tauri::Runtime>(app: AppHandle<R>) -> Reply<String> {
    let resolver = app.path();
    if let Ok(directory) = resolver.download_dir()
        && tokio::fs::try_exists(&directory).await.unwrap_or(false)
    {
        return Ok(directory.to_string_lossy().into_owned());
    }
    resolver
        .home_dir()
        .map(|directory| directory.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

fn absolute_path(path: String) -> Reply<PathBuf> {
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err("an absolute filesystem path is required".into());
    }
    Ok(path)
}

#[tauri::command]
async fn path_exists(path: String) -> Reply<bool> {
    tokio::fs::try_exists(absolute_path(path)?)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_path(app: AppHandle, path: String) -> Reply<()> {
    let path = tokio::fs::canonicalize(absolute_path(path)?)
        .await
        .map_err(|e| e.to_string())?;
    app.opener()
        .open_path(path.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn reveal_path(app: AppHandle, path: String) -> Reply<()> {
    let path = tokio::fs::canonicalize(absolute_path(path)?)
        .await
        .map_err(|e| e.to_string())?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| e.to_string())
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub(crate) fn report(app: &AppHandle, message: String) {
    let _ = app.emit("engine://error", &message);
    app.dialog().message(message).title("OSDM").show(|_| {});
}

fn quit(app: AppHandle) {
    let shell = app.state::<Shell>();
    if shell.quitting.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let shell = app.state::<Shell>();
        match shell.shutdown().await {
            Ok(()) => {
                shell.settings.close().await;
                shell.allow_exit.store(true, Ordering::Release);
                app.exit(0);
            }
            Err(error) => {
                shell.quitting.store(false, Ordering::Release);
                report(
                    &app,
                    format!("Could not save downloads before exit: {error}"),
                );
            }
        }
    });
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_window(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            engine_open,
            engine_shutdown,
            engine_enqueue,
            engine_probe,
            engine_pause,
            engine_resume,
            engine_retry,
            engine_cancel,
            engine_reorder,
            engine_set_speed_limit,
            engine_snapshot,
            engine_remove_download,
            engine_cleanup_partial_data,
            settings_load,
            settings_save,
            pick_directory,
            default_download_directory,
            path_exists,
            open_path,
            reveal_path,
        ])
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let settings =
                tauri::async_runtime::block_on(SettingsStore::open(&directory.join("settings.db")))
                    .map_err(std::io::Error::other)?;
            app.manage(Shell {
                running: Mutex::new(None),
                settings,
                database: directory.join("downloads.db"),
                quitting: AtomicBool::new(false),
                allow_exit: AtomicBool::new(false),
                updating: AtomicBool::new(false),
                tray_available: AtomicBool::new(false),
            });
            let show = MenuItem::with_id(app, "show", "Open OSDM", true, None::<&str>)?;
            let update =
                MenuItem::with_id(app, "update", "Check for updates…", true, None::<&str>)?;
            let exit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &update, &exit])?;
            let tray_available = if let Some(tray_icon) = app.default_window_icon().cloned() {
                match TrayIconBuilder::with_id("osdm")
                    .icon(tray_icon)
                    .tooltip("OSDM — downloads continue while hidden")
                    .menu(&menu)
                    .show_menu_on_left_click(true)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => show_window(app),
                        "quit" => quit(app.clone()),
                        "update" => updates::check(app.clone()),
                        _ => {}
                    })
                    .build(app)
                {
                    Ok(_) => true,
                    Err(error) => {
                        eprintln!("OSDM: system tray unavailable: {error}");
                        false
                    }
                }
            } else {
                eprintln!("OSDM: bundled application icon is missing; system tray unavailable");
                false
            };
            app.state::<Shell>()
                .tray_available
                .store(tray_available, Ordering::Release);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let tray_available = window
                    .state::<Shell>()
                    .tray_available
                    .load(Ordering::Acquire);
                if !tray_available {
                    // Without a tray there is no way back to a hidden window,
                    // so closing must perform a graceful quit instead.
                    quit(window.app_handle().clone());
                    return;
                }
                if let Err(error) = window.hide() {
                    report(
                        window.app_handle(),
                        format!("Could not hide window: {error}"),
                    );
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("could not initialize OSDM desktop shell");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event
            && !app.state::<Shell>().allow_exit.load(Ordering::Acquire)
        {
            api.prevent_exit();
            quit(app.clone());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn ipc_contract_uses_frontend_payload_keys_and_persistent_engine() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("downloads.db");
        let settings =
            tauri::async_runtime::block_on(SettingsStore::open(&dir.path().join("settings.db")))
                .unwrap();
        let app = tauri::test::mock_builder()
            .manage(Shell {
                running: Mutex::new(None),
                settings,
                database: database.clone(),
                quitting: AtomicBool::new(false),
                allow_exit: AtomicBool::new(false),
                updating: AtomicBool::new(false),
                tray_available: AtomicBool::new(false),
            })
            .invoke_handler(tauri::generate_handler![
                engine_open,
                engine_enqueue,
                engine_probe,
                engine_cancel,
                engine_set_speed_limit,
                engine_snapshot,
                engine_remove_download,
                engine_cleanup_partial_data,
                engine_reorder,
                engine_shutdown,
                default_download_directory,
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let invoke = |command: &str, body: Value| {
            tauri::test::get_ipc_response(
                &window,
                tauri::webview::InvokeRequest {
                    cmd: command.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "http://tauri.localhost".parse().unwrap(),
                    body: tauri::ipc::InvokeBody::Json(body),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.into(),
                },
            )
            .map(|response| response.deserialize::<Value>().unwrap())
        };
        let default_dir = invoke("default_download_directory", json!({})).unwrap();
        let default_dir = PathBuf::from(default_dir.as_str().unwrap());
        assert!(default_dir.is_absolute() && default_dir.is_dir());
        invoke("engine_open", json!({"config": Config::default()})).unwrap();
        invoke("engine_open", json!({"config": Config::default()})).unwrap();
        assert!(
            tauri::async_runtime::block_on(Engine::open(&database, Config::default())).is_err()
        );
        invoke("engine_set_speed_limit", json!({"bytesPerSecond": 1024})).unwrap();
        assert!(invoke("engine_set_speed_limit", json!({"bytes_per_second": 1024})).is_err());
        let id = invoke("engine_enqueue", json!({"request": {"url":"http://127.0.0.1:9/file", "destination":dir.path().join("file.bin")}})).unwrap();
        invoke("engine_cancel", json!({"id": id})).unwrap();
        let snapshot = invoke("engine_snapshot", json!({})).unwrap();
        assert_eq!(snapshot[0]["state"], "Canceled");
        assert!(snapshot[0].get("temporary_path").is_some());
        invoke(
            "engine_remove_download",
            json!({"id":id, "removePartialFile":true}),
        )
        .unwrap();
        invoke("engine_reorder", json!({"ids":[]})).unwrap();
        assert_eq!(invoke("engine_cleanup_partial_data", json!({})).unwrap(), 0);
        invoke("engine_shutdown", json!({})).unwrap();
        invoke("engine_shutdown", json!({})).unwrap();
        assert!(invoke("engine_enqueue", json!({"request":{"url":"http://127.0.0.1:9/file","destination":dir.path().join("late.bin")}})).is_err());
        let reopened =
            tauri::async_runtime::block_on(Engine::open(&database, Config::default())).unwrap();
        assert!(tauri::async_runtime::block_on(reopened.snapshot()).is_empty());
        tauri::async_runtime::block_on(reopened.shutdown()).unwrap();
    }
}
