use crate::*;
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::any,
};
use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

struct Server {
    origin: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn serve(router: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { origin, task }
}
async fn terminal(engine: &Engine, id: DownloadId) -> Download {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let download = engine
                .snapshot()
                .await
                .into_iter()
                .find(|d| d.id == id)
                .unwrap();
            if matches!(
                download.state,
                DownloadState::Failed | DownloadState::Completed
            ) {
                return download;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
async fn fixture() -> (tempfile::TempDir, Engine) {
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(
        directory.path().join("site-policy.db"),
        Config {
            max_retries: 0,
            ..Config::default()
        },
    )
    .await
    .unwrap();
    (directory, engine)
}
fn request(server: &Server, directory: &std::path::Path, route: &str) -> DownloadRequest {
    DownloadRequest {
        url: format!("{}{route}", server.origin),
        destination: directory.join("payload.bin"),
        expected_sha256: None,
    }
}

#[tokio::test]
async fn exact_policy_replay_auth_and_identity_edits_are_guarded() {
    let server = serve(Router::new().route("/file", any(|| async { "hello" }))).await;
    let (directory, engine) = fixture().await;
    let id = DownloadId::new_v4();
    let req = request(&server, directory.path(), "/file");
    engine.enqueue_site_child(id, req.clone(), 5).await.unwrap();
    engine.pause(id).await.unwrap();
    assert_eq!(
        engine.enqueue_site_child(id, req.clone(), 5).await.unwrap(),
        id
    );
    assert!(engine.enqueue_site_child(id, req.clone(), 6).await.is_err());
    let mut different = req.clone();
    different.url.push_str("?changed");
    assert!(engine.enqueue_site_child(id, different, 5).await.is_err());
    assert!(engine.set_credentials(id, None).await.is_err());
    assert!(
        engine
            .set_captured_headers(
                id,
                CapturedHeaders {
                    cookie: Some("secret".into()),
                    authorization: None,
                    user_agent: None
                }
            )
            .await
            .is_err()
    );
    assert!(engine.update_url(id, &req.url).await.is_err());
    assert!(
        engine
            .update_destination(id, directory.path().join("different.bin"))
            .await
            .is_err()
    );
    let download = engine.snapshot().await.remove(0);
    assert_eq!(download.max_bytes, Some(5));
    assert!(download.deny_redirects);
    engine.resume(id).await.unwrap();
    assert_eq!(terminal(&engine, id).await.state, DownloadState::Completed);
    // Completed identical admission remains idempotent even though output exists.
    assert_eq!(engine.enqueue_site_child(id, req, 5).await.unwrap(), id);
    assert_eq!(
        tokio::fs::read(download.destination).await.unwrap(),
        b"hello"
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn head_length_is_exact_known_identity_and_zero_is_supported_without_get() {
    let gets = Arc::new(AtomicUsize::new(0));
    let count = gets.clone();
    let server = serve(Router::new().route(
        "/{kind}",
        any(
            move |method: Method, axum::extract::Path(kind): axum::extract::Path<String>| {
                let count = count.clone();
                async move {
                    if method != Method::HEAD {
                        count.fetch_add(1, Ordering::SeqCst);
                    }
                    match kind.as_str() {
                        "unknown" => Body::from_stream(futures_util::stream::once(async {
                            Ok::<_, Infallible>(b"hello".to_vec())
                        }))
                        .into_response(),
                        "encoded" => (
                            [("content-length", "5"), ("content-encoding", "gzip")],
                            Body::empty(),
                        )
                            .into_response(),
                        "zero" => ([("content-length", "0")], Body::empty()).into_response(),
                        _ => (
                            [("content-length", "6")],
                            if method == Method::HEAD {
                                Body::empty()
                            } else {
                                Body::from("123456")
                            },
                        )
                            .into_response(),
                    }
                }
            },
        ),
    ))
    .await;
    for kind in ["oversize", "unknown", "encoded", "zero"] {
        let (directory, engine) = fixture().await;
        let id = DownloadId::new_v4();
        engine
            .enqueue_site_child(
                id,
                request(&server, directory.path(), &format!("/{kind}")),
                if kind == "zero" { 0 } else { 5 },
            )
            .await
            .unwrap();
        let download = terminal(&engine, id).await;
        assert_eq!(
            download.state,
            if kind == "zero" {
                DownloadState::Completed
            } else {
                DownloadState::Failed
            }
        );
        if kind != "zero" {
            assert!(!download.destination.exists());
            assert!(!download.temporary_path.exists());
        } else {
            assert_eq!(
                tokio::fs::metadata(download.destination)
                    .await
                    .unwrap()
                    .len(),
                0
            );
        }
        engine.shutdown().await.unwrap();
    }
    assert_eq!(gets.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn get_length_changes_and_redirects_never_write_or_follow() {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let foreign = serve(Router::new().route(
        "/secret",
        any(move || {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                "secret"
            }
        }),
    ))
    .await;
    let target = format!("{}/secret", foreign.origin);
    let server = serve(Router::new().route(
        "/{kind}",
        any(
            move |method: Method,
                  headers: HeaderMap,
                  axum::extract::Path(kind): axum::extract::Path<String>| {
                let target = target.clone();
                async move {
                    assert!(headers.get("cookie").is_none());
                    assert!(headers.get("authorization").is_none());
                    if kind == "head-redirect" || method != Method::HEAD && kind == "get-redirect" {
                        return (StatusCode::FOUND, [("location", target)], Body::empty())
                            .into_response();
                    }
                    if method == Method::HEAD {
                        ([("content-length", "5")], Body::empty()).into_response()
                    } else if kind == "unknown" {
                        Body::from_stream(futures_util::stream::once(async {
                            Ok::<_, Infallible>(b"hello".to_vec())
                        }))
                        .into_response()
                    } else {
                        "123456".into_response()
                    }
                }
            },
        ),
    ))
    .await;
    for kind in ["changed", "unknown", "head-redirect", "get-redirect"] {
        let (directory, engine) = fixture().await;
        let id = DownloadId::new_v4();
        engine
            .enqueue_site_child(
                id,
                request(&server, directory.path(), &format!("/{kind}")),
                5,
            )
            .await
            .unwrap();
        let download = terminal(&engine, id).await;
        assert_eq!(download.state, DownloadState::Failed);
        assert!(!download.destination.exists());
        assert_eq!(download.downloaded_bytes(), 0);
        if download.temporary_path.exists() {
            assert!(
                tokio::fs::metadata(download.temporary_path)
                    .await
                    .unwrap()
                    .len()
                    <= 5
            );
        }
        engine.shutdown().await.unwrap();
    }
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn durable_payload_cap_allows_checkpointed_range_resume_after_restart() {
    const LENGTH: usize = 512 * 1024;
    let starts = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let recorded = starts.clone();
    let server = serve(Router::new().route(
        "/file",
        any(move |method: Method, headers: HeaderMap| {
            let recorded = recorded.clone();
            async move {
                if method == Method::HEAD {
                    return (
                        [
                            ("content-length", LENGTH.to_string()),
                            ("accept-ranges", "bytes".into()),
                            ("etag", "\"stable\"".into()),
                        ],
                        Body::empty(),
                    )
                        .into_response();
                }
                let range = headers
                    .get("range")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .strip_prefix("bytes=")
                    .unwrap();
                let (start, end) = range.split_once('-').unwrap();
                let start: usize = start.parse().unwrap();
                let end: usize = end.parse().unwrap();
                recorded.lock().await.push(start);
                assert_eq!(headers.get("if-range").unwrap(), "\"stable\"");
                let body = Body::from_stream(futures_util::stream::unfold(
                    start,
                    move |offset| async move {
                        if offset > end {
                            return None;
                        }
                        tokio::time::sleep(Duration::from_millis(8)).await;
                        let count = 8192.min(end + 1 - offset);
                        Some((Ok::<_, Infallible>(vec![0x5a; count]), offset + count))
                    },
                ));
                (
                    StatusCode::PARTIAL_CONTENT,
                    [
                        ("content-length", (end + 1 - start).to_string()),
                        ("content-range", format!("bytes {start}-{end}/{LENGTH}")),
                        ("etag", "\"stable\"".into()),
                    ],
                    body,
                )
                    .into_response()
            }
        }),
    ))
    .await;
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("resume.db");
    let config = Config {
        max_retries: 0,
        connections_per_download: 1,
        ..Config::default()
    };
    let engine = Engine::open(&database, config.clone()).await.unwrap();
    let id = DownloadId::new_v4();
    engine
        .enqueue_site_child(
            id,
            request(&server, directory.path(), "/file"),
            LENGTH as u64,
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while engine.snapshot().await[0].downloaded_bytes() < 32768 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    engine.pause(id).await.unwrap();
    let before = engine.snapshot().await.remove(0);
    let committed = before.downloaded_bytes();
    assert!(committed > 0 && committed < LENGTH as u64);
    engine.shutdown().await.unwrap();
    drop(engine);
    let engine = Engine::open(&database, config).await.unwrap();
    let recovered = engine.snapshot().await.remove(0);
    assert_eq!(recovered.state, DownloadState::Paused);
    assert_eq!(recovered.max_bytes, Some(LENGTH as u64));
    assert!(recovered.deny_redirects);
    assert_eq!(recovered.downloaded_bytes(), committed);
    engine.resume(id).await.unwrap();
    let completed = terminal(&engine, id).await;
    assert_eq!(
        completed.state,
        DownloadState::Completed,
        "{:?}",
        completed.error
    );
    assert!(
        starts
            .lock()
            .await
            .iter()
            .any(|start| *start as u64 == committed)
    );
    assert_eq!(
        tokio::fs::read(completed.destination).await.unwrap(),
        vec![0x5a; LENGTH]
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn schema_six_upgrades_transactionally_and_schema_six_guard_rejects_seven() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("old.db");
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query("PRAGMA user_version=6")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    assert!(engine.list_site_projects().await.is_empty());
    engine.shutdown().await.unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(&database),
    )
    .await
    .unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 10);
    assert!(!(0..=6).contains(&version));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM site_projects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    pool.close().await;
}

#[tokio::test]
async fn failed_schema_seven_migration_rolls_back_every_schema_change() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("conflict.db");
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query("CREATE TABLE downloads(id TEXT PRIMARY KEY, position INTEGER, payload TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE INDEX site_projects ON downloads(position)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("PRAGMA user_version=6")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(Engine::open(&database, Config::default()).await.is_err());
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(&database),
    )
    .await
    .unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 6);
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='automation'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    pool.close().await;
}

#[tokio::test]
async fn selected_stream_retries_are_payload_bounded_not_permanently_escrowed() {
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = calls.clone();
    let server = serve(Router::new().route(
        "/file",
        any(move |method: Method| {
            let attempts = attempts.clone();
            async move {
                if method == Method::HEAD {
                    return (
                        [("content-length", "5"), ("etag", "\"stable\"")],
                        Body::empty(),
                    )
                        .into_response();
                }
                let body = if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    Body::from_stream(futures_util::stream::iter(vec![
                        Ok::<_, std::io::Error>(b"he".to_vec()),
                        Err(std::io::Error::other("test interrupted body")),
                    ]))
                } else {
                    Body::from("hello")
                };
                ([("content-length", "5"), ("etag", "\"stable\"")], body).into_response()
            }
        }),
    ))
    .await;
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(
        directory.path().join("retry.db"),
        Config {
            max_retries: 2,
            ..Config::default()
        },
    )
    .await
    .unwrap();
    let id = DownloadId::new_v4();
    engine
        .enqueue_site_child(id, request(&server, directory.path(), "/file"), 5)
        .await
        .unwrap();
    let download = terminal(&engine, id).await;
    assert_eq!(
        download.state,
        DownloadState::Completed,
        "{:?}",
        download.error
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        tokio::fs::read(download.destination).await.unwrap(),
        b"hello"
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn synchronization_never_unrestrictedly_probes_a_selected_child() {
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = calls.clone();
    let server = serve(Router::new().route(
        "/file",
        any(move || {
            let requests = requests.clone();
            async move {
                requests.fetch_add(1, Ordering::SeqCst);
                "hello"
            }
        }),
    ))
    .await;
    let (directory, engine) = fixture().await;
    let id = DownloadId::new_v4();
    engine
        .enqueue_site_child(id, request(&server, directory.path(), "/file"), 5)
        .await
        .unwrap();
    assert_eq!(terminal(&engine, id).await.state, DownloadState::Completed);
    let before = calls.load(Ordering::SeqCst);
    let queue = DownloadId::new_v4();
    for command in [
        AutomationCommand::SaveQueue {
            id: queue,
            name: "Restricted".into(),
        },
        AutomationCommand::Assign {
            ids: vec![id],
            queue_id: Some(queue),
        },
        AutomationCommand::SetSynchronization {
            id: queue,
            enabled: true,
        },
        AutomationCommand::Start { id: queue },
    ] {
        engine.configure_automation(command).await.unwrap();
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while engine.automation().await.queues[0].checking_sources {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), before);
    assert_eq!(engine.snapshot().await.len(), 1);
    engine.shutdown().await.unwrap();
}
