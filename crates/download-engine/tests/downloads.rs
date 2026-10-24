use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Response},
    routing::get,
};
use download_engine::{Config, Download, DownloadRequest, DownloadState, Engine};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Ranges,
    IgnoreRanges,
    UnknownLength,
    IgnoreAfterProbe,
    WrongRange,
    Retry,
    Disconnect,
    NoValidator,
}

struct ServerState {
    mode: Mode,
    content: Mutex<(Vec<u8>, String)>,
    ranges: Mutex<Vec<String>>,
    requests: AtomicUsize,
}

struct Server {
    url: String,
    state: Arc<ServerState>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server(mode: Mode) -> Server {
    let state = Arc::new(ServerState {
        mode,
        content: Mutex::new((
            (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect(),
            "\"v1\"".into(),
        )),
        ranges: Mutex::new(vec![]),
        requests: AtomicUsize::new(0),
    });
    let router = Router::new()
        .route("/file", get(handler))
        .route(
            "/redirect",
            get(|| async { axum::response::Redirect::temporary("/file") }),
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Server { url, state, task }
}

async fn handler(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response<Body> {
    let request = state.requests.fetch_add(1, Ordering::SeqCst);
    let range = headers
        .get("range")
        .map(|v| v.to_str().unwrap().to_string());
    if let Some(range) = &range {
        state.ranges.lock().unwrap().push(range.clone());
    }
    if state.mode == Mode::Retry && request == 1 {
        return Response::builder()
            .status(429)
            .header("retry-after", "0")
            .body(Body::empty())
            .unwrap();
    }
    let (content, etag) = state.content.lock().unwrap().clone();
    let total = content.len();
    let honor = matches!(
        state.mode,
        Mode::Ranges | Mode::WrongRange | Mode::Retry | Mode::Disconnect | Mode::NoValidator
    ) || (state.mode == Mode::IgnoreAfterProbe
        && range.as_deref() == Some("bytes=0-0"));
    let mut response = Response::builder();
    if state.mode != Mode::NoValidator {
        response = response.header("etag", &etag);
    }
    let bytes = if let Some(range) = range.filter(|_| honor) {
        let (start, end) = range
            .strip_prefix("bytes=")
            .unwrap()
            .split_once('-')
            .unwrap();
        let start: usize = start.parse().unwrap();
        let end: usize = end.parse().unwrap();
        if start >= total {
            return Response::builder()
                .status(416)
                .header("content-range", format!("bytes */{total}"))
                .body(Body::empty())
                .unwrap();
        }
        let reported_start = if state.mode == Mode::WrongRange {
            start + 1
        } else {
            start
        };
        response = response.status(206).header(
            "content-range",
            format!("bytes {reported_start}-{end}/{total}"),
        );
        content[start..=end].to_vec()
    } else {
        response = response.status(200);
        content
    };
    if state.mode != Mode::UnknownLength {
        response = response.header("content-length", bytes.len());
    }
    let disconnect = state.mode == Mode::Disconnect && request == 1;
    let stream = futures_util::stream::unfold((bytes, 0), move |(bytes, offset)| async move {
        if offset >= bytes.len() {
            return None;
        }
        if disconnect && offset >= 64 * 1024 {
            return Some((
                Err::<Vec<u8>, std::io::Error>(std::io::Error::other("connection dropped")),
                (bytes.clone(), bytes.len()),
            ));
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
        let end = (offset + 64 * 1024).min(bytes.len());
        let chunk = bytes[offset..end].to_vec();
        Some((Ok(chunk), (bytes, end)))
    });
    response.body(Body::from_stream(stream)).unwrap()
}

fn config() -> Config {
    Config {
        split_threshold: 128 * 1024,
        max_retries: 1,
        ..Config::default()
    }
}

async fn add(
    engine: &Engine,
    server: &Server,
    dir: &TempDir,
    name: &str,
) -> download_engine::DownloadId {
    engine
        .enqueue(DownloadRequest {
            url: server.url.clone(),
            destination: dir.path().join(name),
        })
        .await
        .unwrap()
}

async fn wait(
    engine: &Engine,
    id: download_engine::DownloadId,
    predicate: impl Fn(&Download) -> bool,
) -> Download {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let download = engine
                .snapshot()
                .await
                .into_iter()
                .find(|d| d.id == id)
                .unwrap();
            if predicate(&download) {
                return download;
            }
            assert_ne!(
                download.state,
                DownloadState::Failed,
                "{:?}",
                download.error
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("download timed out")
}

async fn verify(engine: &Engine, server: &Server, id: download_engine::DownloadId) -> Download {
    let download = wait(engine, id, |d| d.state == DownloadState::Completed).await;
    let result = tokio::fs::read(&download.destination).await.unwrap();
    let expected = server.state.content.lock().unwrap().0.clone();
    assert_eq!(Sha256::digest(&result), Sha256::digest(&expected));
    assert_eq!(download.downloaded_bytes(), expected.len() as u64);
    assert!(!download.temporary_path.exists());
    download
}

#[tokio::test]
async fn segmented_download_has_exact_coverage_and_matching_hash() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "file.bin").await;
    let download = verify(&engine, &server, id).await;
    assert_eq!(download.segments.len(), 4);
    assert_eq!(download.segments[0].start, 0);
    for pair in download.segments.windows(2) {
        assert_eq!(pair[0].end, Some(pair[1].start));
    }
    assert_eq!(download.segments.last().unwrap().end, download.total_bytes);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn unsupported_unknown_and_ignored_ranges_fall_back_safely() {
    for mode in [
        Mode::IgnoreRanges,
        Mode::UnknownLength,
        Mode::IgnoreAfterProbe,
        Mode::NoValidator,
    ] {
        let server = server(mode).await;
        let dir = TempDir::new().unwrap();
        let engine = Engine::open(dir.path().join("queue.db"), config())
            .await
            .unwrap();
        let id = add(&engine, &server, &dir, "file.bin").await;
        let download = verify(&engine, &server, id).await;
        assert_eq!(download.segments.len(), 1);
        engine.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn pause_reopen_and_resume_preserves_durable_ranges() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("queue.db");
    let mut cfg = config();
    cfg.bytes_per_second = 1024 * 1024;
    let engine = Engine::open(&db, cfg).await.unwrap();
    let id = add(&engine, &server, &dir, "file.bin").await;
    wait(&engine, id, |d| d.downloaded_bytes() > 128 * 1024).await;
    engine.pause(id).await.unwrap();
    let paused = engine.snapshot().await.remove(0);
    assert_eq!(paused.state, DownloadState::Paused);
    assert!(paused.downloaded_bytes() > 0);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        engine.snapshot().await[0].downloaded_bytes(),
        paused.downloaded_bytes()
    );
    engine.shutdown().await.unwrap();
    drop(engine);
    let engine = Engine::open(&db, config()).await.unwrap();
    assert_eq!(
        engine.snapshot().await[0].downloaded_bytes(),
        paused.downloaded_bytes()
    );
    server.state.ranges.lock().unwrap().clear();
    engine.resume(id).await.unwrap();
    verify(&engine, &server, id).await;
    let requests = server.state.ranges.lock().unwrap().clone();
    for segment in &paused.segments {
        if segment.committed > 0 && Some(segment.start + segment.committed) != segment.end {
            assert!(requests.contains(&format!(
                "bytes={}-{}",
                segment.start + segment.committed,
                segment.end.unwrap() - 1
            )));
        }
    }
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn changed_remote_file_restarts_instead_of_mixing_bytes() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let mut cfg = config();
    cfg.bytes_per_second = 1024 * 1024;
    let engine = Engine::open(dir.path().join("queue.db"), cfg)
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "file.bin").await;
    wait(&engine, id, |d| d.downloaded_bytes() > 64 * 1024).await;
    engine.pause(id).await.unwrap();
    *server.state.content.lock().unwrap() = (vec![77; 2 * 1024 * 1024], "\"v2\"".into());
    engine.set_speed_limit(0).await;
    engine.resume(id).await.unwrap();
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn retries_rate_limits_and_broken_connections() {
    for mode in [Mode::Retry, Mode::Disconnect] {
        let server = server(mode).await;
        let dir = TempDir::new().unwrap();
        let engine = Engine::open(dir.path().join("queue.db"), config())
            .await
            .unwrap();
        let id = add(&engine, &server, &dir, "file.bin").await;
        verify(&engine, &server, id).await;
        assert!(server.state.requests.load(Ordering::SeqCst) > 5);
        engine.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_wrong_ranges_without_publishing_output() {
    let server = server(Mode::WrongRange).await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "file.bin").await;
    let download = wait(&engine, id, |d| d.state == DownloadState::Failed).await;
    assert!(!download.destination.exists());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn queue_cancel_reorder_and_destination_conflicts() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let mut cfg = config();
    cfg.max_active_downloads = 1;
    cfg.bytes_per_second = 512 * 1024;
    let engine = Engine::open(dir.path().join("queue.db"), cfg)
        .await
        .unwrap();
    let first = add(&engine, &server, &dir, "first.bin").await;
    wait(&engine, first, |d| d.downloaded_bytes() > 0).await;
    let second = add(&engine, &server, &dir, "second.bin").await;
    let third = add(&engine, &server, &dir, "third.bin").await;
    engine.reorder(&[third, second, first]).await.unwrap();
    assert_eq!(engine.snapshot().await[0].id, third);
    assert!(engine.reorder(&[first, first, first]).await.is_err());
    assert!(
        engine
            .enqueue(DownloadRequest {
                url: server.url.clone(),
                destination: dir.path().join("first.bin")
            })
            .await
            .is_err()
    );
    engine.cancel(second).await.unwrap();
    engine.cancel(first).await.unwrap();
    engine.set_speed_limit(0).await;
    verify(&engine, &server, third).await;
    assert_eq!(
        engine
            .snapshot()
            .await
            .iter()
            .find(|d| d.id == second)
            .unwrap()
            .state,
        DownloadState::Canceled
    );
    assert!(!dir.path().join("first.bin").exists());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn existing_destination_is_never_overwritten_even_if_created_mid_transfer() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let mut cfg = config();
    cfg.bytes_per_second = 1024 * 1024;
    let engine = Engine::open(dir.path().join("queue.db"), cfg)
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "file.bin").await;
    wait(&engine, id, |d| d.downloaded_bytes() > 0).await;
    tokio::fs::write(dir.path().join("file.bin"), b"user data")
        .await
        .unwrap();
    engine.set_speed_limit(0).await;
    wait(&engine, id, |d| d.state == DownloadState::Failed).await;
    assert_eq!(
        tokio::fs::read(dir.path().join("file.bin")).await.unwrap(),
        b"user data"
    );
    tokio::fs::remove_file(dir.path().join("file.bin"))
        .await
        .unwrap();
    engine.retry(id).await.unwrap();
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn empty_file_and_redirect_download_correctly() {
    let mut server = server(Mode::Ranges).await;
    *server.state.content.lock().unwrap() = (vec![], "\"empty\"".into());
    server.url = server.url.replace("/file", "/redirect");
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "empty.bin").await;
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}

// Invoked in a child test process by the recovery test. process::exit deliberately
// skips engine shutdown and Rust destructors, leaving SQLite and transfer state live.
#[tokio::test]
async fn process_exit_child() {
    let Ok(directory) = std::env::var("OSDM_CRASH_TEST_DIR") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    let cfg = Config {
        connections_per_download: 1,
        bytes_per_second: 2 * 1024 * 1024,
        ..config()
    };
    let engine = Engine::open(directory.join("queue.db"), cfg).await.unwrap();
    let id = engine
        .enqueue(DownloadRequest {
            url: std::env::var("OSDM_CRASH_TEST_URL").unwrap(),
            destination: directory.join("crash.bin"),
        })
        .await
        .unwrap();
    wait(&engine, id, |d| {
        d.segments.iter().any(|s| s.committed >= 1024 * 1024)
    })
    .await;
    std::process::exit(23);
}

#[tokio::test]
async fn abrupt_process_exit_recovers_committed_progress() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "process_exit_child", "--nocapture"])
        .env("OSDM_CRASH_TEST_DIR", dir.path())
        .env("OSDM_CRASH_TEST_URL", &server.url);
    let status = tokio::task::spawn_blocking(move || command.status().unwrap())
        .await
        .unwrap();
    assert_eq!(status.code(), Some(23));
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let download = engine.snapshot().await.remove(0);
    assert_eq!(download.state, DownloadState::Paused);
    assert!(download.downloaded_bytes() >= 1024 * 1024);
    assert!(download.downloaded_bytes() < download.total_bytes.unwrap());
    engine.resume(download.id).await.unwrap();
    verify(&engine, &server, download.id).await;
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn database_has_exclusive_ownership_until_shutdown() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("queue.db");
    let first = Engine::open(&db, config()).await.unwrap();
    assert!(Engine::open(&db, config()).await.is_err());
    first.shutdown().await.unwrap();
    let second = Engine::open(&db, config()).await.unwrap();
    first.shutdown().await.unwrap();
    assert!(Engine::open(&db, config()).await.is_err());
    second.shutdown().await.unwrap();
}

#[tokio::test]
async fn bandwidth_setting_changes_wake_existing_waits() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let cfg = Config {
        bytes_per_second: 1,
        ..config()
    };
    let engine = Engine::open(dir.path().join("queue.db"), cfg)
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "file.bin").await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while server.state.requests.load(Ordering::SeqCst) < 5 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(engine.snapshot().await[0].downloaded_bytes(), 0);
    engine.set_speed_limit(0).await;
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}
