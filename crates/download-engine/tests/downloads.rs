use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Response},
    routing::get,
};
#[cfg(debug_assertions)]
use download_engine::test_faults;
use download_engine::{Config, Download, DownloadRequest, DownloadState, Engine};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tempfile::TempDir;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Ranges,
    IgnoreRanges,
    UnknownLength,
    IgnoreAfterProbe,
    WrongRange,
    Retry,
    RetryDate,
    RetryMany,
    Disconnect,
    TruncateRepeatedly,
    NoValidator,
}

struct ServerState {
    mode: Mode,
    content: Mutex<(Vec<u8>, String)>,
    ranges: Mutex<Vec<String>>,
    requests: AtomicUsize,
    active: AtomicUsize,
    max_active: AtomicUsize,
    truncated: AtomicUsize,
}

struct ActiveGuard(Arc<ServerState>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
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
        active: AtomicUsize::new(0),
        max_active: AtomicUsize::new(0),
        truncated: AtomicUsize::new(0),
    });
    let router = Router::new()
        .route("/file", get(handler))
        .route(
            "/redirect",
            get(|| async { axum::response::Redirect::temporary("/file") }),
        )
        .route(
            "/loop",
            get(|| async { axum::response::Redirect::temporary("/loop") }),
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Server { url, state, task }
}

struct LargeServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for LargeServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct LargeState {
    size: u64,
}

async fn large_server(size: u64) -> LargeServer {
    let state = Arc::new(LargeState { size });
    let router = Router::new()
        .route("/large", get(large_handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/large", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    LargeServer { url, task }
}

async fn large_handler(State(state): State<Arc<LargeState>>, headers: HeaderMap) -> Response<Body> {
    let size = state.size;
    let range = headers.get("range").and_then(|value| value.to_str().ok());
    let (status, start, end) = match range {
        Some(range) => {
            let Some(value) = range.strip_prefix("bytes=") else {
                return range_not_satisfiable(size);
            };
            let Some((start, end)) = value.split_once('-') else {
                return range_not_satisfiable(size);
            };
            let Ok(start) = start.parse::<u64>() else {
                return range_not_satisfiable(size);
            };
            let end = if end.is_empty() {
                size.saturating_sub(1)
            } else {
                let Ok(end) = end.parse::<u64>() else {
                    return range_not_satisfiable(size);
                };
                end
            };
            if start >= size || end < start {
                return range_not_satisfiable(size);
            }
            (206, start, end.min(size - 1))
        }
        None => (200, 0, size.saturating_sub(1)),
    };
    let length = end.saturating_sub(start) + 1;
    let mut response = Response::builder()
        .status(status)
        .header("etag", "\"large\"")
        .header("accept-ranges", "bytes")
        .header("content-length", length.to_string());
    if status == 206 {
        response = response.header("content-range", format!("bytes {start}-{end}/{size}"));
    }
    let stream = futures_util::stream::unfold((start, end), |(position, end)| async move {
        if position > end {
            return None;
        }
        let length = (end - position + 1).min(64 * 1024) as usize;
        let chunk = vec![0u8; length];
        Some((
            Ok::<_, std::io::Error>(chunk),
            (position + length as u64, end),
        ))
    });
    response.body(Body::from_stream(stream)).unwrap()
}

fn range_not_satisfiable(size: u64) -> Response<Body> {
    Response::builder()
        .status(416)
        .header("content-range", format!("bytes */{size}"))
        .body(Body::empty())
        .unwrap()
}

struct ProbeServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for ProbeServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn probe_server() -> ProbeServer {
    let router = Router::new().route("/named", get(named_handler)).route(
        "/redirect",
        get(|| async { axum::response::Redirect::temporary("/named") }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/named", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    ProbeServer { url, task }
}

async fn named_handler(headers: HeaderMap) -> Response<Body> {
    let ranged = headers
        .get("range")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "bytes=0-0");
    let mut response = Response::builder()
        .header("etag", "\"named\"")
        .header(
            "content-disposition",
            "attachment; filename=\"server-name.zip\"",
        )
        .header("content-type", "application/zip; charset=binary");
    if ranged {
        response = response
            .status(206)
            .header("content-range", "bytes 0-0/12345")
            .header("content-length", "1");
        response.body(Body::from(vec![b'x'])).unwrap()
    } else {
        response
            .status(200)
            .header("content-length", "12345")
            .body(Body::empty())
            .unwrap()
    }
}

async fn handler(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response<Body> {
    let request = state.requests.fetch_add(1, Ordering::SeqCst);
    let active = state.active.fetch_add(1, Ordering::SeqCst) + 1;
    state.max_active.fetch_max(active, Ordering::SeqCst);
    let guard = ActiveGuard(state.clone());
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
    if state.mode == Mode::RetryDate && request == 1 {
        // Two seconds in the future so the integer-second HTTP-date is always
        // more than one second away, even when the current time is near a
        // second boundary.
        let when = httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(2));
        return Response::builder()
            .status(429)
            .header("retry-after", when)
            .body(Body::empty())
            .unwrap();
    }
    if state.mode == Mode::RetryMany && request < 2 {
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
        Mode::Ranges
            | Mode::WrongRange
            | Mode::Retry
            | Mode::Disconnect
            | Mode::TruncateRepeatedly
            | Mode::NoValidator
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
    let truncate_repeatedly = state.mode == Mode::TruncateRepeatedly && bytes.len() > 1536 * 1024;
    let disconnect = (state.mode == Mode::Disconnect && request == 1) || truncate_repeatedly;
    if truncate_repeatedly {
        state.truncated.fetch_add(1, Ordering::SeqCst);
    }
    let fail_after = if truncate_repeatedly {
        1536 * 1024
    } else {
        64 * 1024
    };
    let stream_delay = if state.mode == Mode::TruncateRepeatedly {
        Duration::from_millis(10)
    } else {
        Duration::from_millis(2)
    };
    let stream = futures_util::stream::unfold(
        (bytes, 0, guard),
        move |(bytes, offset, guard)| async move {
            if offset >= bytes.len() {
                return None;
            }
            if disconnect && offset >= fail_after {
                return Some((
                    Err::<Vec<u8>, std::io::Error>(std::io::Error::other("connection dropped")),
                    (bytes.clone(), bytes.len(), guard),
                ));
            }
            tokio::time::sleep(stream_delay).await;
            let end = (offset + 64 * 1024).min(bytes.len());
            let chunk = bytes[offset..end].to_vec();
            Some((Ok(chunk), (bytes, end, guard)))
        },
    );
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
            expected_sha256: None,
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
async fn probe_reports_server_filename_without_enqueueing() {
    let server = probe_server().await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();

    let info = engine.probe(&server.url).await.unwrap();

    assert_eq!(info.final_url, server.url);
    assert_eq!(info.filename.as_deref(), Some("server-name.zip"));
    assert_eq!(info.content_type.as_deref(), Some("application/zip"));
    assert_eq!(info.total_bytes, Some(12345));
    assert!(info.ranged);

    let redirected = engine
        .probe(&server.url.replace("/named", "/redirect"))
        .await
        .unwrap();
    assert_eq!(redirected.final_url, server.url);
    assert_eq!(redirected.filename.as_deref(), Some("server-name.zip"));

    assert!(engine.snapshot().await.is_empty());
    engine.shutdown().await.unwrap();
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
async fn resumed_segment_layout_obeys_lower_current_connection_ceiling() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("queue.db");
    let initial = Config {
        connections_per_download: 4,
        bytes_per_second: 1024 * 1024,
        ..config()
    };
    let engine = Engine::open(&db, initial).await.unwrap();
    let id = add(&engine, &server, &dir, "lower-ceiling.bin").await;
    wait(&engine, id, |d| d.downloaded_bytes() > 64 * 1024).await;
    engine.pause(id).await.unwrap();
    let paused = engine.snapshot().await.remove(0);
    assert_eq!(
        paused.segments.len(),
        4,
        "fixture needs a four-lane saved layout"
    );
    engine.shutdown().await.unwrap();
    drop(engine);

    server.state.active.store(0, Ordering::SeqCst);
    server.state.max_active.store(0, Ordering::SeqCst);
    let resumed_config = Config {
        connections_per_download: 1,
        ..config()
    };
    let engine = Engine::open(&db, resumed_config).await.unwrap();
    engine.resume(id).await.unwrap();
    verify(&engine, &server, id).await;
    let observed = server.state.max_active.load(Ordering::SeqCst);
    assert!(
        observed <= 1,
        "resumed download exceeded its current one-connection cap: {observed}"
    );
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
async fn checksum_mismatch_is_not_published_and_retry_redownloads_for_verification() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let intended = vec![91; 2 * 1024 * 1024];
    let expected = format!("{:x}", Sha256::digest(&intended));
    let id = engine
        .enqueue(DownloadRequest {
            url: server.url.clone(),
            destination: dir.path().join("verified.bin"),
            expected_sha256: Some(expected.clone()),
        })
        .await
        .unwrap();
    let failed = wait(&engine, id, |d| d.state == DownloadState::Failed).await;
    assert_eq!(failed.integrity, download_engine::IntegrityStatus::Mismatch);
    assert!(
        !failed.destination.exists(),
        "mismatched bytes were published"
    );

    *server.state.content.lock().unwrap() = (intended.clone(), "\"v2\"".into());
    engine.retry(id).await.unwrap();
    let completed = verify(&engine, &server, id).await;
    assert_eq!(
        completed.integrity,
        download_engine::IntegrityStatus::Verified
    );
    assert_eq!(completed.sha256.as_deref(), Some(expected.as_str()));
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn expected_checksum_persists_across_restart_and_is_verified() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("queue.db");
    let expected = {
        let content = server.state.content.lock().unwrap().0.clone();
        format!("{:x}", Sha256::digest(content))
    };
    let mut cfg = config();
    cfg.bytes_per_second = 1024 * 1024;
    let engine = Engine::open(&db, cfg).await.unwrap();
    let id = engine
        .enqueue(DownloadRequest {
            url: server.url.clone(),
            destination: dir.path().join("verified-restart.bin"),
            expected_sha256: Some(expected.clone()),
        })
        .await
        .unwrap();
    wait(&engine, id, |download| {
        download.downloaded_bytes() > 64 * 1024
    })
    .await;
    engine.pause(id).await.unwrap();
    engine.shutdown().await.unwrap();
    drop(engine);

    let engine = Engine::open(&db, config()).await.unwrap();
    assert_eq!(
        engine.snapshot().await[0].expected_sha256.as_deref(),
        Some(expected.as_str())
    );
    engine.resume(id).await.unwrap();
    let completed = verify(&engine, &server, id).await;
    assert_eq!(
        completed.integrity,
        download_engine::IntegrityStatus::Verified
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn congestion_adapts_ranged_subrequests_without_gaps_or_overlap() {
    let server = server(Mode::Retry).await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "adaptive.bin").await;
    verify(&engine, &server, id).await;
    let size = server.state.content.lock().unwrap().0.len() as u64;
    let ranges = server.state.ranges.lock().unwrap();
    let requests: Vec<(u64, u64)> = ranges
        .iter()
        .filter(|range| range.as_str() != "bytes=0-0")
        .map(|range| {
            let (start, end) = range
                .strip_prefix("bytes=")
                .unwrap()
                .split_once('-')
                .unwrap();
            (start.parse().unwrap(), end.parse::<u64>().unwrap() + 1)
        })
        .collect();
    assert!(!requests.is_empty());
    for (start, end) in requests {
        assert!(start < size, "request started beyond resource end");
        assert!(
            end - start <= 512 * 1024,
            "adaptive chunk exceeded reduced size"
        );
    }
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn retries_rate_limits_and_broken_connections() {
    for (mode, minimum_wait) in [
        // Retry-After: 0 must still back off exponentially instead of hammering.
        (Mode::Retry, Duration::from_millis(900)),
        (Mode::Disconnect, Duration::ZERO),
    ] {
        let server = server(mode).await;
        let dir = TempDir::new().unwrap();
        let engine = Engine::open(dir.path().join("queue.db"), config())
            .await
            .unwrap();
        let started = Instant::now();
        let id = add(&engine, &server, &dir, "file.bin").await;
        verify(&engine, &server, id).await;
        assert!(
            started.elapsed() >= minimum_wait,
            "{mode:?} retried too early"
        );
        assert!(server.state.requests.load(Ordering::SeqCst) > 5);
        engine.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn repeated_truncations_reset_retry_budget_after_checkpointed_progress() {
    let server = server(Mode::TruncateRepeatedly).await;
    let content: Vec<u8> = (0..32 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    *server.state.content.lock().unwrap() = (content, "\"large-v1\"".into());
    let dir = TempDir::new().unwrap();
    let cfg = Config {
        connections_per_download: 1,
        max_retries: 1,
        ..config()
    };
    let engine = Engine::open(dir.path().join("queue.db"), cfg)
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "truncated-retries.bin").await;
    let completed = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let download = engine
                .snapshot()
                .await
                .into_iter()
                .find(|download| download.id == id)
                .unwrap();
            if download.state == DownloadState::Completed {
                break download;
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
    .expect("repeated truncations prevented recovery");
    let output = tokio::fs::read(&completed.destination).await.unwrap();
    let expected = server.state.content.lock().unwrap().0.clone();
    assert_eq!(Sha256::digest(&output), Sha256::digest(expected));
    assert!(
        server.state.truncated.load(Ordering::SeqCst) > 1,
        "expected more truncations than the configured retry budget"
    );
    assert_eq!(completed.downloaded_bytes(), 32 * 1024 * 1024);

    let ranges = server.state.ranges.lock().unwrap();
    let starts: Vec<u64> = ranges
        .iter()
        .filter(|range| range.as_str() != "bytes=0-0")
        .map(|range| {
            range
                .strip_prefix("bytes=")
                .unwrap()
                .split_once('-')
                .unwrap()
                .0
                .parse()
                .unwrap()
        })
        .collect();
    assert!(starts.len() > 2, "expected multiple ranged subrequests");
    assert_eq!(starts[0], 0);
    assert!(
        starts.windows(2).all(|pair| pair[1] > pair[0]),
        "subrequests restarted or failed to advance at saved offsets: {starts:?}"
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn persistent_rate_limits_retry_beyond_configured_retry_count() {
    let server = server(Mode::RetryMany).await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "file.bin").await;
    verify(&engine, &server, id).await;
    assert!(server.state.requests.load(Ordering::SeqCst) >= 3);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn update_url_persists_and_reuses_checkpoint_for_same_representation() {
    let original = server(Mode::Ranges).await;
    let replacement = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("queue.db");
    let mut cfg = config();
    cfg.bytes_per_second = 1024 * 1024;
    let engine = Engine::open(&db, cfg).await.unwrap();
    let id = add(&engine, &original, &dir, "file.bin").await;
    wait(&engine, id, |d| d.downloaded_bytes() > 64 * 1024).await;
    engine.pause(id).await.unwrap();
    let before = engine.snapshot().await.remove(0);
    engine.update_url(id, &replacement.url).await.unwrap();
    let updated = engine.snapshot().await.remove(0);
    assert_eq!(updated.state, DownloadState::Paused);
    assert_eq!(updated.url, replacement.url);
    assert_eq!(updated.downloaded_bytes(), before.downloaded_bytes());
    engine.shutdown().await.unwrap();

    let engine = Engine::open(&db, config()).await.unwrap();
    let persisted = engine.snapshot().await.remove(0);
    assert_eq!(persisted.url, replacement.url);
    assert_eq!(persisted.downloaded_bytes(), before.downloaded_bytes());
    replacement.state.ranges.lock().unwrap().clear();
    engine.resume(id).await.unwrap();
    verify(&engine, &replacement, id).await;
    let requests = replacement.state.ranges.lock().unwrap();
    assert!(persisted.segments.iter().any(|segment| {
        segment.committed > 0
            && Some(segment.start + segment.committed) != segment.end
            && requests.contains(&format!(
                "bytes={}-{}",
                segment.start + segment.committed,
                segment.end.unwrap() - 1
            ))
    }));
    drop(requests);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn update_url_rejects_invalid_url_and_active_or_queued_downloads() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "file.bin").await;
    assert!(engine.update_url(id, "not a URL").await.is_err());
    assert!(engine.update_url(id, &server.url).await.is_err());
    engine.pause(id).await.unwrap();
    engine.update_url(id, &server.url).await.unwrap();
    engine.resume(id).await.unwrap();
    let completed = verify(&engine, &server, id).await;
    assert!(engine.update_url(id, &server.url).await.is_err());
    assert!(completed.downloaded_bytes() > 0);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn honors_http_date_retry_after() {
    let server = server(Mode::RetryDate).await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let started = Instant::now();
    let id = add(&engine, &server, &dir, "file.bin").await;
    verify(&engine, &server, id).await;
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "HTTP-date Retry-After was not honored"
    );
    engine.shutdown().await.unwrap();
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
                destination: dir.path().join("first.bin"),
                expected_sha256: None,
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

#[tokio::test]
async fn redirect_loops_fail_without_publishing() {
    let mut server = server(Mode::Ranges).await;
    server.url = server.url.replace("/file", "/loop");
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "loop.bin").await;

    let failed = wait(&engine, id, |d| d.state == DownloadState::Failed).await;
    assert!(failed.error.is_some());
    assert!(!failed.destination.exists());
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
            expected_sha256: None,
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

#[cfg(debug_assertions)]
#[tokio::test]
async fn disk_full_at_checkpoint_does_not_publish() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "disk-full.bin").await;
    test_faults::fail_checkpoint(id);

    let failed = wait(&engine, id, |d| d.state == DownloadState::Failed).await;
    assert!(
        failed
            .error
            .as_deref()
            .is_some_and(|error| error.contains("injected disk full"))
    );
    assert!(!failed.destination.exists());
    assert!(failed.temporary_path.exists());

    test_faults::clear(id);
    engine.retry(id).await.unwrap();
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn readonly_temporary_file_fails_and_retries() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let cfg = Config {
        bytes_per_second: 1024 * 1024,
        ..config()
    };
    let engine = Engine::open(dir.path().join("queue.db"), cfg)
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "readonly.bin").await;
    let download = wait(&engine, id, |d| {
        d.downloaded_bytes() > 0 && d.temporary_path.exists()
    })
    .await;
    engine.pause(id).await.unwrap();

    let original_permissions = std::fs::metadata(&download.temporary_path)
        .unwrap()
        .permissions();
    let mut readonly_permissions = original_permissions.clone();
    readonly_permissions.set_readonly(true);
    std::fs::set_permissions(&download.temporary_path, readonly_permissions).unwrap();

    engine.resume(id).await.unwrap();
    let failed = wait(&engine, id, |d| d.state == DownloadState::Failed).await;
    assert!(!failed.destination.exists());

    std::fs::set_permissions(&download.temporary_path, original_permissions).unwrap();

    engine.retry(id).await.unwrap();
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn cancellation_during_checkpoint_drains_and_preserves_progress() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let cfg = Config {
        bytes_per_second: 512 * 1024,
        ..config()
    };
    let engine = Engine::open(dir.path().join("queue.db"), cfg)
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "checkpoint-cancel.bin").await;
    let gate = test_faults::block_checkpoint(id);

    tokio::time::timeout(Duration::from_secs(10), gate.wait_until_blocked())
        .await
        .expect("checkpoint gate was not reached");
    let cancel = tokio::spawn({
        let engine = engine.clone();
        async move { engine.cancel(id).await }
    });
    wait(&engine, id, |d| d.state == DownloadState::Canceling).await;
    gate.release();
    cancel.await.unwrap().unwrap();

    let canceled = wait(&engine, id, |d| d.state == DownloadState::Canceled).await;
    assert!(!canceled.destination.exists());
    assert!(canceled.downloaded_bytes() > 0);

    test_faults::clear(id);
    engine.resume(id).await.unwrap();
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn cancellation_during_hashing_preserves_checkpoints_and_can_resume() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let expected = {
        let content = server.state.content.lock().unwrap().0.clone();
        format!("{:x}", Sha256::digest(content))
    };
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = engine
        .enqueue(DownloadRequest {
            url: server.url.clone(),
            destination: dir.path().join("hash-cancel.bin"),
            expected_sha256: Some(expected),
        })
        .await
        .unwrap();
    let gate = test_faults::block_hashing(id);
    tokio::time::timeout(Duration::from_secs(20), gate.wait_until_blocked())
        .await
        .expect("hashing gate was not reached");
    let verifying = engine.snapshot().await.remove(0);
    assert_eq!(
        verifying.integrity,
        download_engine::IntegrityStatus::Verifying
    );

    tokio::time::timeout(Duration::from_secs(5), engine.cancel(id))
        .await
        .expect("cancel did not interrupt hashing")
        .unwrap();
    let canceled = wait(&engine, id, |d| d.state == DownloadState::Canceled).await;
    assert!(!canceled.destination.exists());
    assert!(canceled.downloaded_bytes() > 0);
    assert!(
        canceled
            .segments
            .iter()
            .all(|segment| segment.downloaded == segment.committed)
    );

    engine.resume(id).await.unwrap();
    let completed = verify(&engine, &server, id).await;
    assert_eq!(
        completed.integrity,
        download_engine::IntegrityStatus::Verified
    );
    engine.shutdown().await.unwrap();
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn cancellation_before_publication_prevents_link() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let cfg = Config {
        bytes_per_second: 512 * 1024,
        ..config()
    };
    let engine = Engine::open(dir.path().join("queue.db"), cfg)
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "publish-cancel.bin").await;
    let gate = test_faults::block_publication_entry(id);
    engine.set_speed_limit(0).await;

    tokio::time::timeout(Duration::from_secs(5), gate.wait_until_blocked())
        .await
        .expect("publication entry gate was not reached");
    let cancel = tokio::spawn({
        let engine = engine.clone();
        async move { engine.cancel(id).await }
    });
    wait(&engine, id, |d| d.state == DownloadState::Canceling).await;
    gate.release();
    cancel.await.unwrap().unwrap();

    let canceled = wait(&engine, id, |d| d.state == DownloadState::Canceled).await;
    assert!(!canceled.destination.exists());

    test_faults::clear(id);
    engine.resume(id).await.unwrap();
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn cancellation_after_ready_marker_completes_safely() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let cfg = Config {
        bytes_per_second: 512 * 1024,
        ..config()
    };
    let engine = Engine::open(dir.path().join("queue.db"), cfg)
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "ready-cancel.bin").await;
    let gate = test_faults::block_publication(id);
    engine.set_speed_limit(0).await;

    tokio::time::timeout(Duration::from_secs(5), gate.wait_until_blocked())
        .await
        .expect("publication gate was not reached");
    let cancel = tokio::spawn({
        let engine = engine.clone();
        async move { engine.cancel(id).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    gate.release();
    cancel.await.unwrap().unwrap();

    let download = wait(&engine, id, |d| d.state == DownloadState::Completed).await;
    assert!(download.destination.exists());
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn publication_failure_after_ready_marker_retries() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "publish-failure.bin").await;
    test_faults::fail_publication(id);

    let failed = wait(&engine, id, |d| d.state == DownloadState::Failed).await;
    assert!(
        failed
            .error
            .as_deref()
            .is_some_and(|error| error.contains("injected publication failure"))
    );
    assert!(!failed.destination.exists());
    assert!(failed.temporary_path.exists());

    test_faults::clear(id);
    engine.retry(id).await.unwrap();
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn corrupted_publication_recovery_is_rejected_then_redownloaded() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let expected = {
        let content = server.state.content.lock().unwrap().0.clone();
        format!("{:x}", Sha256::digest(content))
    };
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = engine
        .enqueue(DownloadRequest {
            url: server.url.clone(),
            destination: dir.path().join("recovery.bin"),
            expected_sha256: Some(expected),
        })
        .await
        .unwrap();
    test_faults::fail_publication(id);
    let failed = wait(&engine, id, |d| d.state == DownloadState::Failed).await;
    assert!(!failed.destination.exists());
    assert!(failed.temporary_path.exists());
    assert_eq!(failed.integrity, download_engine::IntegrityStatus::Verified);

    tokio::fs::write(&failed.temporary_path, vec![0xA5; 2 * 1024 * 1024])
        .await
        .unwrap();
    engine.retry(id).await.unwrap();
    let mismatch = wait(&engine, id, |d| {
        d.state == DownloadState::Failed
            && d.integrity == download_engine::IntegrityStatus::Mismatch
    })
    .await;
    assert!(
        !mismatch.destination.exists(),
        "corrupt recovery was published"
    );

    engine.retry(id).await.unwrap();
    let completed = verify(&engine, &server, id).await;
    assert_eq!(
        completed.integrity,
        download_engine::IntegrityStatus::Verified
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn connection_caps_are_enforced() {
    for (max_global, max_host, downloads) in [(2, 8, 2), (8, 2, 1)] {
        let server = server(Mode::Ranges).await;
        let dir = TempDir::new().unwrap();
        let cfg = Config {
            max_active_downloads: downloads,
            max_connections: max_global,
            max_connections_per_host: max_host,
            bytes_per_second: 2 * 1024 * 1024,
            ..config()
        };
        let engine = Engine::open(dir.path().join("queue.db"), cfg)
            .await
            .unwrap();
        let mut ids = Vec::new();
        for index in 0..downloads {
            ids.push(add(&engine, &server, &dir, &format!("cap-{index}.bin")).await);
        }
        for id in ids {
            verify(&engine, &server, id).await;
        }

        let observed = server.state.max_active.load(Ordering::SeqCst);
        let expected_cap = max_global.min(max_host);
        assert!(
            observed <= expected_cap,
            "observed {observed} concurrent requests with cap {expected_cap}"
        );
        assert!(
            observed >= 2,
            "expected parallel requests, observed only {observed}"
        );
        engine.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_drains_promptly() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let cfg = Config {
        bytes_per_second: 512 * 1024,
        ..config()
    };
    let engine = Engine::open(dir.path().join("queue.db"), cfg)
        .await
        .unwrap();
    let id = add(&engine, &server, &dir, "cancel-latency.bin").await;
    wait(&engine, id, |d| d.downloaded_bytes() > 0).await;

    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), engine.cancel(id))
        .await
        .expect("cancel did not drain within two seconds")
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));

    let canceled = wait(&engine, id, |d| d.state == DownloadState::Canceled).await;
    assert!(!canceled.destination.exists());
    engine.shutdown().await.unwrap();
}

async fn create_database(path: &Path, version: i64, payload: Option<&str>) {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS downloads (id TEXT PRIMARY KEY, position INTEGER NOT NULL, payload TEXT NOT NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();
    if let Some(payload) = payload {
        sqlx::query("INSERT INTO downloads (id, position, payload) VALUES ('test', 0, ?)")
            .bind(payload)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query(&format!("PRAGMA user_version = {version}"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

async fn database_version(path: &Path) -> i64 {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    version
}

#[tokio::test]
async fn schema_initializes_and_rejects_newer_or_corrupt_databases() {
    let dir = TempDir::new().unwrap();

    let fresh = dir.path().join("fresh.db");
    let engine = Engine::open(&fresh, config()).await.unwrap();
    engine.shutdown().await.unwrap();
    drop(engine);
    assert_eq!(database_version(&fresh).await, 3);

    let unversioned = dir.path().join("unversioned.db");
    create_database(&unversioned, 0, None).await;
    let engine = Engine::open(&unversioned, config()).await.unwrap();
    engine.shutdown().await.unwrap();
    drop(engine);
    assert_eq!(database_version(&unversioned).await, 3);

    let revision_two = dir.path().join("revision-two.db");
    create_database(&revision_two, 2, None).await;
    let engine = Engine::open(&revision_two, config()).await.unwrap();
    engine.shutdown().await.unwrap();
    drop(engine);
    assert_eq!(database_version(&revision_two).await, 3);

    let legacy = dir.path().join("legacy-download.db");
    let payload = serde_json::json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "url": "http://example.test/file",
        "destination": dir.path().join("legacy.bin"),
        "temporary_path": dir.path().join("legacy.part"),
        "state": "Paused",
        "total_bytes": null,
        "segments": [],
        "error": null,
        "bytes_per_second": 0,
        "eta_seconds": null,
        "validator": null,
        "ranged": false,
        "ready_to_publish": false
    });
    create_database(&legacy, 2, Some(&payload.to_string())).await;
    let engine = Engine::open(&legacy, config()).await.unwrap();
    let migrated = engine.snapshot().await.remove(0);
    assert_eq!(migrated.expected_sha256, None);
    assert_eq!(migrated.sha256, None);
    assert_eq!(
        migrated.integrity,
        download_engine::IntegrityStatus::Unverified
    );
    assert!(migrated.adaptive.is_none());
    engine.shutdown().await.unwrap();
    drop(engine);
    assert_eq!(database_version(&legacy).await, 3);

    let newer = dir.path().join("newer.db");
    create_database(&newer, 4, None).await;
    let error = match Engine::open(&newer, config()).await {
        Ok(_) => panic!("newer schema was accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("newer"));

    let corrupt = dir.path().join("corrupt.db");
    create_database(&corrupt, 1, Some("not json")).await;
    let error = match Engine::open(&corrupt, config()).await {
        Ok(_) => panic!("corrupt payload was accepted"),
        Err(error) => error,
    };
    assert!(matches!(error, download_engine::Error::Serialization(_)));
}

#[cfg(windows)]
#[tokio::test]
async fn windows_long_path_download() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let mut parent = dir.path().to_path_buf();
    for index in 0..4 {
        parent.push(format!("{}-{index}", "long-directory-name-".repeat(4)));
    }
    std::fs::create_dir_all(&parent).unwrap();
    let destination = parent.join(format!("{}.bin", "long-file-name-".repeat(10)));
    assert!(destination.to_string_lossy().len() > 260);

    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = engine
        .enqueue(DownloadRequest {
            url: server.url.clone(),
            destination,
            expected_sha256: None,
        })
        .await
        .unwrap();
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn windows_unicode_destination() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let id = engine
        .enqueue(DownloadRequest {
            url: server.url.clone(),
            destination: dir.path().join("ダウンロード-测试-🚀.bin"),
            expected_sha256: None,
        })
        .await
        .unwrap();
    verify(&engine, &server, id).await;
    engine.shutdown().await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn windows_case_insensitive_destination_is_reserved() {
    let server = server(Mode::Ranges).await;
    let dir = TempDir::new().unwrap();
    let engine = Engine::open(dir.path().join("queue.db"), config())
        .await
        .unwrap();
    let first = add(&engine, &server, &dir, "Case.bin").await;
    let duplicate = engine
        .enqueue(DownloadRequest {
            url: server.url.clone(),
            destination: dir.path().join("case.bin"),
            expected_sha256: None,
        })
        .await;
    assert!(duplicate.is_err());
    assert_eq!(engine.snapshot().await.len(), 1);

    verify(&engine, &server, first).await;
    let existing = engine
        .enqueue(DownloadRequest {
            url: server.url.clone(),
            destination: dir.path().join("CASE.BIN"),
            expected_sha256: None,
        })
        .await;
    assert!(existing.is_err());
    engine.shutdown().await.unwrap();
}

#[cfg(windows)]
fn working_set() -> std::io::Result<usize> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    // SAFETY: the handle is the current process and the struct is fully initialized by the API.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(counters.WorkingSetSize)
    }
}

#[cfg(windows)]
fn available_space(path: &Path) -> std::io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0u64;
    // SAFETY: the path is NUL-terminated and the out-pointer is valid for one u64.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(available)
    }
}

#[cfg(windows)]
#[ignore = "requires OSDM_LARGE_TEST=1, about 2.1 GiB free disk space, and several minutes"]
#[tokio::test]
async fn multi_gigabyte_download_keeps_memory_bounded() {
    if std::env::var_os("OSDM_LARGE_TEST").is_none() {
        eprintln!("set OSDM_LARGE_TEST=1 to run the multi-gigabyte download test");
        return;
    }
    const SIZE: u64 = 2 * 1024 * 1024 * 1024 + 12_345;

    let server = large_server(SIZE).await;
    let dir = TempDir::new().unwrap();
    if available_space(dir.path()).unwrap() < 3 * 1024 * 1024 * 1024 {
        eprintln!("skipping large download test: less than 3 GiB free");
        return;
    }
    let cfg = Config {
        connections_per_download: 4,
        split_threshold: 64 * 1024 * 1024,
        ..config()
    };
    let engine = Engine::open(dir.path().join("queue.db"), cfg)
        .await
        .unwrap();
    let id = engine
        .enqueue(DownloadRequest {
            url: server.url.clone(),
            destination: dir.path().join("large.bin"),
            expected_sha256: None,
        })
        .await
        .unwrap();

    let baseline = working_set().unwrap();
    let mut peak = baseline;
    let deadline = Instant::now() + Duration::from_secs(900);
    let download = loop {
        let download = engine
            .snapshot()
            .await
            .into_iter()
            .find(|download| download.id == id)
            .unwrap();
        peak = peak.max(working_set().unwrap());
        if download.state == DownloadState::Completed {
            break download;
        }
        assert_ne!(
            download.state,
            DownloadState::Failed,
            "{:?}",
            download.error
        );
        assert!(Instant::now() < deadline, "large download timed out");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };

    assert_eq!(download.total_bytes, Some(SIZE));
    assert_eq!(download.downloaded_bytes(), SIZE);
    assert_eq!(
        std::fs::metadata(&download.destination).unwrap().len(),
        SIZE
    );
    let growth = peak.saturating_sub(baseline);
    assert!(
        growth < 128 * 1024 * 1024,
        "working set grew by {growth} bytes"
    );

    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut file = tokio::fs::File::open(&download.destination).await.unwrap();
    for offset in [0, SIZE / 2, SIZE - 1024] {
        file.seek(std::io::SeekFrom::Start(offset)).await.unwrap();
        let mut buffer = [0u8; 256];
        file.read_exact(&mut buffer).await.unwrap();
        assert!(buffer.iter().all(|byte| *byte == 0));
    }

    let expected = {
        let mut hasher = Sha256::new();
        let zeros = [0u8; 64 * 1024];
        let mut remaining = SIZE;
        while remaining > 0 {
            let length = remaining.min(zeros.len() as u64) as usize;
            hasher.update(&zeros[..length]);
            remaining -= length as u64;
        }
        hasher.finalize()
    };
    file.seek(std::io::SeekFrom::Start(0)).await.unwrap();
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let length = file.read(&mut buffer).await.unwrap();
        if length == 0 {
            break;
        }
        hasher.update(&buffer[..length]);
    }
    assert_eq!(hasher.finalize(), expected);

    engine.shutdown().await.unwrap();
}
