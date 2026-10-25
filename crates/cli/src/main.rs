use download_engine::{
    AutomationCommand, Config, DownloadId, DownloadRequest, DownloadState, Engine,
};
use serde_json::{Value, json};
use std::{ffi::OsString, io::Write, path::PathBuf, time::Duration};

const HELP: &str = "OSDM CLI (JSON Lines output; no prompts)
Usage:
  osdm-cli add --database PATH --directory PATH [--filename NAME] [--queue-only] [--input FILE] [--silent] [URL ...]
  osdm-cli start-queue --database PATH [--silent]
  osdm-cli start-queue --database PATH --queue UUID
  osdm-cli stop-queue --database PATH --queue UUID
  osdm-cli queues --database PATH
  osdm-cli configure --database PATH --command JSON_FILE
  osdm-cli run-scheduler --database PATH
  osdm-cli start-queue --desktop --queue UUID
  osdm-cli stop-queue --desktop --queue UUID
  osdm-cli queues --desktop
Input files: UTF-8, one HTTP(S) URL per line; blank lines ignored.
The database must not be open in the desktop app or another CLI process.
Exit codes: 0 success, 1 operation/transfer failure, 2 invalid input, 130 interrupted.
";

struct Options {
    start: bool,
    mode: String,
    queue: Option<DownloadId>,
    command: Option<AutomationCommand>,
    desktop: bool,
    database: PathBuf,
    requests: Vec<DownloadRequest>,
    queue_only: bool,
}

fn text(value: OsString) -> Result<String, &'static str> {
    value.into_string().map_err(|_| "expected UTF-8 text")
}

// Reject unsafe names rather than silently rewriting them and introducing collisions.
fn filename(name: &str) -> Result<(), &'static str> {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("unsafe filename; supply --filename for a single URL");
    }
    Ok(())
}

fn parse(args: impl Iterator<Item = OsString>) -> Result<Options, &'static str> {
    let mut args = args;
    let mode = text(args.next().ok_or("expected a command; use --help")?)?;
    if ![
        "add",
        "start-queue",
        "stop-queue",
        "queues",
        "configure",
        "run-scheduler",
    ]
    .contains(&mode.as_str())
    {
        return Err("unknown command; use --help");
    }
    let start = mode != "add";
    let mut queue = None;
    let mut command = None;
    let mut desktop = false;
    let (mut database, mut directory, mut name) = (None, None, None);
    let mut urls = Vec::new();
    let mut queue_only = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--database") => {
                database = Some(PathBuf::from(args.next().ok_or("missing database path")?))
            }
            Some("--directory") if !start => {
                directory = Some(PathBuf::from(args.next().ok_or("missing directory path")?))
            }
            Some("--filename") if !start => {
                name = Some(text(args.next().ok_or("missing filename")?)?)
            }
            Some("--queue-only") if !start => queue_only = true,
            Some("--silent") => {} // All output is machine-readable, with no progress chatter.
            Some("--desktop")
                if ["start-queue", "stop-queue", "queues"].contains(&mode.as_str()) =>
            {
                desktop = true
            }
            Some("--queue") if mode == "start-queue" || mode == "stop-queue" || mode == "add" => {
                queue = Some(
                    text(args.next().ok_or("missing queue UUID")?)?
                        .parse()
                        .map_err(|_| "invalid queue UUID")?,
                );
            }
            Some("--command") if mode == "configure" => {
                let input = std::fs::read_to_string(args.next().ok_or("missing command file")?)
                    .map_err(|_| "cannot read command file")?;
                command = Some(
                    serde_json::from_str(&input).map_err(|_| "invalid automation command JSON")?,
                );
            }
            Some("--input") if !start => {
                let path = args.next().ok_or("missing input file")?;
                let input =
                    std::fs::read_to_string(path).map_err(|_| "cannot read UTF-8 input file")?;
                urls.extend(
                    input
                        .trim_start_matches('\u{feff}')
                        .lines()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned),
                );
            }
            Some(value) if !start && !value.starts_with('-') => urls.push(value.to_owned()),
            _ => return Err("unknown option or unexpected argument"),
        }
    }
    let database = match (database, desktop) {
        (Some(_), true) => return Err("choose --desktop or --database, not both"),
        (Some(path), false) => path,
        (None, false) => return Err("--database is required"),
        (None, true) => PathBuf::new(),
    };
    if desktop && mode != "queues" && queue.is_none() {
        return Err("--desktop queue commands require --queue");
    }
    if mode == "configure" && command.is_none() {
        return Err("--command is required");
    }
    if mode == "stop-queue" && queue.is_none() {
        return Err("--queue is required");
    }
    let mut requests = Vec::new();
    if !start {
        if urls.is_empty() || (name.is_some() && urls.len() != 1) {
            return Err("provide URLs; --filename requires exactly one URL");
        }
        let directory = std::fs::canonicalize(directory.ok_or("--directory is required")?)
            .map_err(|_| "destination directory must exist")?;
        if !directory.is_dir() {
            return Err("destination directory must be a directory");
        }
        for raw in urls {
            let mut url = reqwest::Url::parse(&raw).map_err(|_| "invalid HTTP(S) URL")?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err("expected HTTP(S) URL without embedded credentials");
            }
            url.set_fragment(None);
            let derived =
                percent_encoding::percent_decode_str(url.path().rsplit('/').next().unwrap_or(""))
                    .decode_utf8()
                    .map_err(|_| "filename is not UTF-8")?;
            let file = name.as_deref().unwrap_or(&derived);
            filename(file)?;
            requests.push(DownloadRequest {
                url: url.to_string(),
                destination: directory.join(file),
                expected_sha256: None,
            });
        }
    }
    Ok(Options {
        start,
        mode,
        queue,
        command,
        desktop,
        database,
        requests,
        queue_only,
    })
}

fn emit(value: Value) -> Result<(), &'static str> {
    writeln!(std::io::stdout().lock(), "{value}").map_err(|_| "output_failed")
}

fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

async fn execute(engine: &Engine, options: &Options) -> Result<u8, &'static str> {
    if options.mode == "queues" {
        emit(json!({"type":"automation", "state":engine.automation().await}))?;
        return Ok(0);
    }
    if let Some(command) = &options.command {
        let state = engine
            .configure_automation(command.clone())
            .await
            .map_err(|_| "automation_failed")?;
        emit(json!({"type":"automation", "state":state}))?;
        return Ok(0);
    }
    if options.mode == "stop-queue" {
        engine
            .configure_automation(AutomationCommand::Stop {
                id: options.queue.unwrap(),
            })
            .await
            .map_err(|_| "automation_failed")?;
        return Ok(0);
    }
    let mut ids = Vec::<DownloadId>::new();
    let mut failed = false;
    if options.start {
        if let Some(queue) = options.queue {
            engine
                .configure_automation(AutomationCommand::Start { id: queue })
                .await
                .map_err(|_| "automation_failed")?;
        }
        let state = engine.automation().await;
        ids.extend(
            engine
                .snapshot()
                .await
                .iter()
                .filter(|d| matches!(d.state, DownloadState::Paused | DownloadState::Queued))
                .filter(|d| !state.pending_captures.contains_key(&d.id))
                .filter(|d| {
                    options
                        .queue
                        .is_none_or(|q| state.membership.get(&d.id) == Some(&q))
                })
                .map(|d| d.id),
        );
    } else {
        if let Some(queue) = options.queue {
            let items = options
                .requests
                .iter()
                .map(|request| download_engine::BatchItem {
                    request: request.clone(),
                    queue_id: Some(queue),
                })
                .collect();
            for result in engine
                .enqueue_batch(items, true)
                .await
                .map_err(|_| "enqueue_failed")?
            {
                if let Some(id) = result.id
                    && !ids.contains(&id)
                {
                    ids.push(id);
                }
                if result.error.is_some() {
                    failed = true;
                    emit(json!({"type":"error", "code":"enqueue_failed", "index":result.index}))?;
                } else {
                    emit(
                        json!({"type":"accepted", "index":result.index, "id":result.id, "duplicate":result.duplicate}),
                    )?;
                }
            }
        } else {
            for (index, request) in options.requests.iter().enumerate() {
                let existing = engine.snapshot().await.into_iter().find(|d| {
                    same_path(&d.destination, &request.destination) && d.url == request.url
                });
                let (id, duplicate) = if let Some(download) = existing {
                    (download.id, true)
                } else {
                    match engine.enqueue_paused(request.clone()).await {
                        Ok(id) => (id, false),
                        Err(_) => {
                            failed = true;
                            emit(json!({"type":"error", "code":"enqueue_failed", "index": index}))?;
                            continue;
                        }
                    }
                };
                emit(json!({"type":"accepted", "index":index, "id":id, "duplicate":duplicate}))?;
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    if options.queue_only {
        return Ok(u8::from(failed));
    }
    for id in &ids {
        if options.mode == "run-scheduler" {
            break;
        }
        engine.resume(*id).await.map_err(|_| "resume_failed")?;
    }
    let interrupted = tokio::signal::ctrl_c();
    tokio::pin!(interrupted);
    let named_queue = options.queue.filter(|_| options.mode == "start-queue");
    let mut reported = std::collections::HashSet::new();
    while !ids.is_empty() || options.mode == "run-scheduler" || named_queue.is_some() {
        let snapshot = engine.snapshot().await;
        let automation = engine.automation().await;
        if let Some(queue) = named_queue {
            for d in &snapshot {
                if automation.membership.get(&d.id) == Some(&queue)
                    && !reported.contains(&d.id)
                    && !ids.contains(&d.id)
                {
                    ids.push(d.id);
                }
            }
        }
        let mut pending = Vec::new();
        for id in &ids {
            let Some(download) = snapshot.iter().find(|d| d.id == *id) else {
                failed = true;
                continue;
            };
            if matches!(
                download.state,
                DownloadState::Completed | DownloadState::Failed | DownloadState::Canceled
            ) {
                failed |= download.state != DownloadState::Completed;
                emit(json!({"type":"finished", "id":id, "state":download.state}))?;
                reported.insert(*id);
            } else {
                pending.push(*id);
            }
        }
        ids = pending;
        let queue_busy = named_queue.is_some_and(|id| {
            automation
                .queues
                .iter()
                .any(|q| q.id == id && (q.running || q.checking_sources))
        });
        if named_queue.is_some() && !queue_busy && options.mode != "run-scheduler" {
            failed |= !ids.is_empty();
            break;
        }
        if ids.is_empty() && !queue_busy && options.mode != "run-scheduler" {
            break;
        }
        tokio::select! {
            _ = &mut interrupted => return Ok(130),
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }
    Ok(u8::from(failed))
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--help" {
        print!("{HELP}");
        return std::process::ExitCode::SUCCESS;
    }
    let options = match parse(args.into_iter()) {
        Ok(options) => options,
        Err(message) => {
            let _ = emit(json!({"type":"error", "code":"invalid_input", "message":message}));
            return 2.into();
        }
    };
    if options.desktop {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap();
        let message = match options.mode.as_str() {
            "start-queue" => json!({"op":"start_queue", "id":options.queue}),
            "stop-queue" => json!({"op":"stop_queue", "id":options.queue}),
            _ => json!({"op":"queues"}),
        };
        return match osdm_browser_bridge::forward(&client, &message).await {
            Some(reply) if reply["ok"] == true => {
                if emit(json!({"type":"automation", "state":reply["state"]})).is_ok() {
                    0.into()
                } else {
                    1.into()
                }
            }
            _ => {
                let _ = emit(
                    json!({"type":"error", "code":"desktop_unavailable", "message":"Open OSDM and enable local browser integration, then retry or inspect the queue"}),
                );
                1.into()
            }
        };
    }
    let opened = if options.mode == "run-scheduler" {
        Engine::open(&options.database, Config::default()).await
    } else {
        Engine::open_for_commands(&options.database, Config::default()).await
    };
    let engine = match opened {
        Ok(engine) => engine,
        Err(_) => {
            let _ = emit(
                json!({"type":"error", "code":"engine_open_failed", "message":"Cannot open database; ensure its parent exists and no other process owns it"}),
            );
            return 1.into();
        }
    };
    let result = execute(&engine, &options).await;
    // Always drain and persist, including after partial admission or an error.
    if engine.shutdown().await.is_err() {
        let _ = emit(json!({"type":"error", "code":"shutdown_failed"}));
        return 1.into();
    }
    match result {
        Ok(code) => {
            if code == 130 {
                let _ = emit(json!({"type":"error", "code":"interrupted"}));
            }
            code.into()
        }
        Err(code) => {
            let _ = emit(json!({"type":"error", "code":code}));
            1.into()
        }
    }
}
