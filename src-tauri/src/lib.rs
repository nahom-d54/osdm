mod capture;
mod completion;
mod power;
mod settings;
mod startup;
mod updates;

use download_engine::{
    BasicCredentials, Config, Download, DownloadId, DownloadProperties, DownloadRequest,
    DownloadState, Engine, EngineEvent, ProbeInfo,
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
        let mut actions = tokio::task::JoinSet::new();
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
                while actions.try_join_next().is_some() {}
                if download.state == DownloadState::Completed && completed.insert(download.id) {
                    let name = download
                        .destination
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    let preferences = app
                        .state::<Shell>()
                        .settings
                        .load()
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or_default()
                        .desktop;
                    if !preferences.silent
                        && let Err(error) = app
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
                    // Bounded native action tasks do not stall snapshot delivery.
                    let handle = app.clone();
                    let file = download.destination.clone();
                    if actions.len() >= 8 {
                        let _ = app.emit("engine://error", "Completion action could not run: eight scanner/sound actions are already active. The completed file is kept; scan it manually.");
                        continue;
                    }
                    actions.spawn(async move {
                        if let Err(error) = completion::scan(&preferences, &file).await {
                            let _ = handle.emit("engine://error", error);
                        }
                        if !preferences.silent
                            && !preferences.sound_file.is_empty()
                            && let Err(error) =
                                completion::sound(std::path::Path::new(&preferences.sound_file))
                                    .await
                        {
                            let _ = handle.emit("engine://error", error);
                        }
                    });
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
async fn engine_enqueue(
    shell: State<'_, Shell>,
    request: DownloadRequest,
    start_paused: Option<bool>,
    properties: Option<DownloadProperties>,
    credentials: Option<BasicCredentials>,
) -> Reply<DownloadId> {
    let engine = shell.engine().await?;
    let settings = shell.settings.load().await?.unwrap_or_default();
    let temporary_directory = (!settings.temporary_directory.is_empty())
        .then(|| PathBuf::from(settings.temporary_directory));
    let result = engine
        .enqueue_with_properties(
            request,
            properties.unwrap_or_default(),
            start_paused.unwrap_or(false),
            credentials,
            temporary_directory,
        )
        .await;
    result.map_err(|e| e.to_string())
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

#[tauri::command]
async fn engine_automation(shell: State<'_, Shell>) -> Reply<download_engine::Automation> {
    Ok(shell.engine().await?.automation().await)
}

#[tauri::command]
async fn engine_configure_automation(
    shell: State<'_, Shell>,
    command: download_engine::AutomationCommand,
) -> Reply<download_engine::Automation> {
    shell
        .engine()
        .await?
        .configure_automation(command)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn engine_enqueue_batch(
    shell: State<'_, Shell>,
    items: Vec<download_engine::BatchItem>,
    start_paused: bool,
) -> Reply<Vec<download_engine::BatchResult>> {
    shell
        .engine()
        .await?
        .enqueue_batch(items, start_paused)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn engine_update_properties(
    shell: State<'_, Shell>,
    id: DownloadId,
    properties: DownloadProperties,
) -> Reply<()> {
    shell
        .engine()
        .await?
        .update_properties(id, properties)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn engine_update_destination(
    shell: State<'_, Shell>,
    id: DownloadId,
    destination: PathBuf,
) -> Reply<()> {
    shell
        .engine()
        .await?
        .update_destination(id, destination)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn engine_set_credentials(
    shell: State<'_, Shell>,
    id: DownloadId,
    credentials: Option<BasicCredentials>,
) -> Reply<()> {
    shell
        .engine()
        .await?
        .set_credentials(id, credentials)
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
async fn engine_update_url(shell: State<'_, Shell>, id: DownloadId, url: String) -> Reply<()> {
    shell
        .engine()
        .await?
        .update_url(id, &url)
        .await
        .map_err(|e| e.to_string())
}

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
async fn updates_check(app: AppHandle) -> Reply<()> {
    updates::check(app);
    Ok(())
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
async fn test_sound(path: String) -> Reply<()> {
    completion::sound(&absolute_path(path)?).await
}

#[tauri::command]
async fn drag_completed(
    window: tauri::WebviewWindow,
    shell: State<'_, Shell>,
    id: DownloadId,
) -> Reply<()> {
    let download = shell
        .engine()
        .await?
        .snapshot()
        .await
        .into_iter()
        .find(|download| download.id == id && download.state == DownloadState::Completed)
        .ok_or("only a completed download can be dragged out")?;
    let path = tokio::fs::canonicalize(download.destination)
        .await
        .map_err(|_| "completed file is unavailable")?;
    if !path.is_file() {
        return Err("completed file is not a regular file".into());
    }
    let native_window = window.clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    window
        .run_on_main_thread(move || {
            #[cfg(target_os = "linux")]
            let result = native_window
                .gtk_window()
                .map_err(|error| error.to_string())
                .and_then(|window| {
                    drag::start_drag(
                        &window,
                        drag::DragItem::Files(vec![path]),
                        drag::Image::Raw(include_bytes!("../icons/32x32.png").to_vec()),
                        |_, _| {},
                        drag::Options::default(),
                    )
                    .map_err(|error| error.to_string())
                });
            #[cfg(not(target_os = "linux"))]
            let result = drag::start_drag(
                &native_window,
                drag::DragItem::Files(vec![path]),
                drag::Image::Raw(include_bytes!("../icons/32x32.png").to_vec()),
                |_, _| {},
                drag::Options::default(),
            )
            .map_err(|error| error.to_string());
            let _ = send.send(result);
        })
        .map_err(|error| error.to_string())?;
    receive
        .await
        .map_err(|_| "drag operation ended unexpectedly")?
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
    let test_directory = osdm_browser_bridge::native_test_directory().unwrap_or_else(|_| {
        eprintln!("OSDM: invalid native test isolation directory");
        std::process::exit(2);
    });
    if std::env::args().any(|arg| arg == "--native-test-info") {
        println!(
            "{}",
            serde_json::json!({"native_test":test_directory.is_some()})
        );
        return;
    }
    let test_mode = test_directory.is_some();
    let mut context = tauri::generate_context!();
    if let Some(directory) = &test_directory {
        use sha2::{Digest, Sha256};
        let identity = format!(
            "{:x}",
            Sha256::digest(directory.to_string_lossy().as_bytes())
        );
        context.config_mut().identifier =
            format!("app.osdm.desktop.native-test-{}", &identity[..16]);
        // Create the disposable webview explicitly below so even its Chromium
        // cache/localStorage stays inside the marked test directory.
        context.config_mut().app.windows.clear();
    }
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_window(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    // These plugins are unnecessary in isolated tests and may register native OS
    // integrations. Test fixtures never enable real notifications or updates.
    let builder = if test_mode {
        builder
    } else {
        builder
            .plugin(tauri_plugin_notification::init())
            .plugin(tauri_plugin_updater::Builder::new().build())
    };
    let app = builder
        .invoke_handler(tauri::generate_handler![
            engine_open,
            engine_shutdown,
            engine_enqueue,
            engine_enqueue_batch,
            engine_automation,
            engine_configure_automation,
            capture::capture_status,
            capture::capture_enable,
            power::power_status,
            power::power_arm,
            power::power_cancel,
            power::power_acknowledge,
            startup::desktop_autostart,
            startup::desktop_autostart_status,
            test_sound,
            drag_completed,
            engine_update_properties,
            engine_set_credentials,
            engine_update_destination,
            engine_probe,
            engine_pause,
            engine_resume,
            engine_retry,
            engine_update_url,
            engine_cancel,
            engine_reorder,
            engine_set_speed_limit,
            engine_snapshot,
            engine_remove_download,
            engine_cleanup_partial_data,
            settings_load,
            settings_save,
            updates_check,
            pick_directory,
            default_download_directory,
            path_exists,
            open_path,
            reveal_path,
        ])
        .setup(move |app| {
            let directory = match &test_directory {
                Some(directory) => directory.clone(),
                None => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&directory)?;
            let settings =
                tauri::async_runtime::block_on(SettingsStore::open(&directory.join("settings.db")))
                    .map_err(std::io::Error::other)?;
            if test_mode {
                std::fs::create_dir_all(directory.join("downloads"))?;
                tauri::async_runtime::block_on(async {
                    if settings.load().await?.is_none() {
                        let config = Settings {
                            default_directory: directory
                                .join("downloads")
                                .to_string_lossy()
                                .into_owned(),
                            onboarding_complete: true,
                            auto_update_check: false,
                            desktop: settings::DesktopPreferences {
                                silent: true,
                                completion_dialog: false,
                                ..Default::default()
                            },
                            ..Default::default()
                        };
                        settings.save(&config).await?;
                    }
                    Ok::<(), String>(())
                })
                .map_err(std::io::Error::other)?;
            }
            app.manage(Shell {
                running: Mutex::new(None),
                settings,
                database: directory.join("downloads.db"),
                quitting: AtomicBool::new(false),
                allow_exit: AtomicBool::new(false),
                updating: AtomicBool::new(false),
                tray_available: AtomicBool::new(false),
            });
            if test_mode {
                tauri::WebviewWindowBuilder::new(
                    app,
                    "main",
                    tauri::WebviewUrl::App("index.html".into()),
                )
                .title("OSDM — isolated native test")
                .data_directory(directory.join("webview"))
                .inner_size(1024.0, 700.0)
                .build()?;
            }
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
            if !test_mode {
                updates::start_auto_check(app.handle().clone());
            }
            capture::start(app.handle(), directory);
            power::start(app.handle());
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
        .build(context)
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
            .manage(power::PowerState::default())
            .invoke_handler(tauri::generate_handler![
                engine_open,
                engine_enqueue,
                engine_enqueue_batch,
                engine_automation,
                engine_configure_automation,
                power::power_arm,
                power::power_cancel,
                power::power_status,
                engine_update_properties,
                engine_set_credentials,
                engine_update_destination,
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
        // A local origin is required for app commands to bypass the IPC ACL.
        // Windows serves the custom protocol as http://tauri.localhost; Linux
        // and macOS use tauri://localhost.
        let origin = if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        };
        let invoke = |command: &str, body: Value| {
            tauri::test::get_ipc_response(
                &window,
                tauri::webview::InvokeRequest {
                    cmd: command.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: origin.parse().unwrap(),
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
        let queue = DownloadId::new_v4();
        invoke(
            "engine_configure_automation",
            json!({"command":{"action":"save_queue","id":queue,"name":"IPC queue"}}),
        )
        .unwrap();
        let batch = invoke("engine_enqueue_batch", json!({"items":[{"request":{"url":"http://127.0.0.1:9/batch","destination":dir.path().join("batch.bin")},"queue_id":queue}],"startPaused":true})).unwrap();
        let batch_id = &batch[0]["id"];
        assert!(batch[0]["error"].is_null());
        assert_eq!(
            invoke("engine_automation", json!({})).unwrap()["membership"]
                [batch_id.as_str().unwrap()],
            queue.to_string()
        );
        assert!(
            invoke(
                "power_arm",
                json!({"queueId":queue,"action":{"kind":"shutdown"},"consent":false})
            )
            .is_err()
        );
        if cfg!(windows) {
            invoke(
                "power_arm",
                json!({"queueId":queue,"action":{"kind":"shutdown"},"consent":true}),
            )
            .unwrap();
            assert!(invoke("power_status", json!({})).unwrap()["deadline"].is_null());
        }
        invoke("power_cancel", json!({})).unwrap();
        assert!(invoke("power_status", json!({})).unwrap().is_null());
        invoke("engine_cancel", json!({"id":batch_id})).unwrap();
        invoke(
            "engine_remove_download",
            json!({"id":batch_id,"removePartialFile":true}),
        )
        .unwrap();
        assert_eq!(invoke("engine_cleanup_partial_data", json!({})).unwrap(), 0);
        let deferred = invoke("engine_enqueue", json!({"request":{"url":"http://127.0.0.1:9/file","destination":dir.path().join("deferred.bin")},"startPaused":true})).unwrap();
        assert_eq!(
            invoke("engine_snapshot", json!({})).unwrap()[0]["state"],
            "Paused"
        );
        invoke("engine_update_properties", json!({"id":deferred,"properties":{"description":"Saved through IPC","category_id":"documents"}})).unwrap();
        invoke(
            "engine_update_destination",
            json!({"id":deferred,"destination":dir.path().join("renamed.bin")}),
        )
        .unwrap();
        invoke(
            "engine_set_credentials",
            json!({"id":deferred,"credentials":{"username":"test","password":"session-only"}}),
        )
        .unwrap();
        let updated = invoke("engine_snapshot", json!({})).unwrap();
        assert_eq!(updated[0]["properties"]["description"], "Saved through IPC");
        assert_eq!(updated[0]["properties"]["category_id"], "documents");
        assert!(!updated.to_string().contains("session-only"));
        invoke("engine_cancel", json!({"id":deferred})).unwrap();
        invoke(
            "engine_remove_download",
            json!({"id":deferred,"removePartialFile":false}),
        )
        .unwrap();
        invoke("engine_shutdown", json!({})).unwrap();
        invoke("engine_shutdown", json!({})).unwrap();
        assert!(invoke("engine_enqueue", json!({"request":{"url":"http://127.0.0.1:9/file","destination":dir.path().join("late.bin")}})).is_err());
        let reopened =
            tauri::async_runtime::block_on(Engine::open(&database, Config::default())).unwrap();
        assert!(tauri::async_runtime::block_on(reopened.snapshot()).is_empty());
        tauri::async_runtime::block_on(reopened.shutdown()).unwrap();
    }
}
