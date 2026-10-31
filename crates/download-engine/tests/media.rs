use axum::{
    Router,
    body::Body,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use download_engine::{
    Config, Engine, Error,
    media_capture::{self, Manifest},
};
use futures_util::stream;
use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

const MASTER: &str =
    "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nlow/list.m3u8\n";
const VOD: &str = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:5,\na.ts\n#EXT-X-ENDLIST\n";

struct Server {
    origin: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server(router: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { origin, task }
}

async fn engine(config: Config) -> (tempfile::TempDir, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("media.db"), config)
        .await
        .unwrap();
    (dir, engine)
}

async fn inspect(engine: &Engine, source: &str) -> download_engine::Result<Manifest> {
    tokio::time::timeout(
        Duration::from_secs(5),
        engine.inspect_hls(source, &[], &CancellationToken::new()),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn resolves_against_final_url_without_fetching_choices_or_segments() {
    let server = server(
        Router::new()
            .route(
                "/start",
                get(|| async { (StatusCode::FOUND, [("location", "/media/master.m3u8")]) }),
            )
            .route("/media/master.m3u8", get(|| async { MASTER }))
            .route("/media/low/list.m3u8", get(|| async { VOD })),
    )
    .await;
    let (_dir, engine) = engine(Config::default()).await;
    let Manifest::Master { variants } = inspect(&engine, &format!("{}/start", server.origin))
        .await
        .unwrap()
    else {
        panic!("expected master")
    };
    assert_eq!(variants[0].resolution, Some((640, 360)));
    assert_eq!(
        variants[0].source.as_str(),
        format!("{}/media/low/list.m3u8", server.origin)
    );
    let Manifest::Vod { segments } = inspect(&engine, variants[0].source.as_str()).await.unwrap()
    else {
        panic!("expected VOD")
    };
    assert_eq!(
        segments[0].source.as_str(),
        format!("{}/media/low/a.ts", server.origin)
    );
    assert!(engine.snapshot().await.is_empty());
    assert_eq!(
        std::fs::read_dir(_dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "part"))
            .count(),
        0
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn cross_origin_redirect_requires_explicit_approval_before_request() {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let target = server(Router::new().route(
        "/master",
        get(move |headers: HeaderMap| {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                for header in ["cookie", "authorization", "referer", "range"] {
                    assert!(!headers.contains_key(header));
                }
                MASTER
            }
        }),
    ))
    .await;
    let destination = format!("{}/master", target.origin);
    let source = server(Router::new().route(
        "/start",
        get(move || {
            let destination = destination.clone();
            async move { (StatusCode::FOUND, [("location", destination)]) }
        }),
    ))
    .await;
    let (_dir, engine) = engine(Config::default()).await;
    let url = format!("{}/start", source.origin);
    assert!(matches!(
        inspect(&engine, &url).await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    engine
        .inspect_hls(
            &url,
            std::slice::from_ref(&target.origin),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_unsafe_sources_and_noncanonical_allowlists_without_network() {
    let (_dir, engine) = engine(Config::default()).await;
    for source in [
        "file:///manifest",
        "http://user:password@127.0.0.1/index",
        "http://127.0.0.1/index#fragment",
        " http://127.0.0.1/index",
    ] {
        assert!(matches!(
            inspect(&engine, source).await,
            Err(Error::Media(_))
        ));
    }
    for allowed in [
        "http://127.0.0.1/path",
        "http://127.0.0.1/",
        "http://127.0.0.1?query",
        "https://user:password@example.test",
    ] {
        assert!(
            engine
                .inspect_hls(
                    "http://127.0.0.1:1/manifest",
                    &[allowed.into()],
                    &CancellationToken::new()
                )
                .await
                .is_err()
        );
    }
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn redirect_loops_and_excessive_chains_terminate() {
    let server = server(
        Router::new()
            .route(
                "/loop",
                get(|| async { (StatusCode::FOUND, [("location", "/loop")]) }),
            )
            .route(
                "/chain/{n}",
                get(
                    |axum::extract::Path(n): axum::extract::Path<usize>| async move {
                        (
                            StatusCode::FOUND,
                            [("location", format!("/chain/{}", n + 1))],
                        )
                    },
                ),
            ),
    )
    .await;
    let (_dir, engine) = engine(Config::default()).await;
    for path in ["/loop", "/chain/0"] {
        assert!(matches!(
            inspect(&engine, &format!("{}{path}", server.origin)).await,
            Err(Error::Invalid(_))
        ));
    }
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn bounds_declared_and_chunked_bodies_and_rejects_encoding_partial_and_invalid_documents() {
    let server = server(
        Router::new()
            .route(
                "/declared",
                get(|| async { "x".repeat(media_capture::MAX_MANIFEST_BYTES + 1) }),
            )
            .route(
                "/chunked",
                get(|| async {
                    Body::from_stream(stream::iter(
                        (0..17).map(|_| Ok::<_, Infallible>(vec![b'x'; 65536])),
                    ))
                }),
            )
            .route(
                "/encoded",
                get(|| async { ([("content-encoding", "gzip")], MASTER) }),
            )
            .route(
                "/partial",
                get(|| async { (StatusCode::PARTIAL_CONTENT, MASTER) }),
            )
            .route("/invalid", get(|| async { "not a playlist" }))
            .route(
                "/live",
                get(|| async { "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:5,\na.ts\n" }),
            ),
    )
    .await;
    let (_dir, engine) = engine(Config::default()).await;
    for path in ["/declared", "/chunked"] {
        assert!(matches!(
            inspect(&engine, &format!("{}{path}", server.origin)).await,
            Err(Error::Media(media_capture::Error::Limit))
        ));
    }
    assert!(matches!(
        inspect(&engine, &format!("{}/encoded", server.origin)).await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        inspect(&engine, &format!("{}/partial", server.origin)).await,
        Err(Error::Http(206))
    ));
    assert!(matches!(
        inspect(&engine, &format!("{}/invalid", server.origin)).await,
        Err(Error::Media(media_capture::Error::Invalid))
    ));
    assert!(matches!(
        inspect(&engine, &format!("{}/live", server.origin)).await,
        Err(Error::Media(media_capture::Error::Unsupported))
    ));
    engine.shutdown().await.unwrap();
}

fn stalled_body(entered: Arc<Notify>) -> Response {
    let body = stream::once(async move {
        entered.notify_one();
        std::future::pending::<Result<Vec<u8>, Infallible>>().await
    });
    Body::from_stream(body).into_response()
}

#[tokio::test]
async fn cancellation_releases_body_reads_and_both_connection_budgets() {
    let entered = Arc::new(Notify::new());
    let notification = entered.clone();
    let server = server(
        Router::new()
            .route(
                "/stall",
                get(move || {
                    let notification = notification.clone();
                    async move { stalled_body(notification) }
                }),
            )
            .route("/ok", get(|| async { MASTER })),
    )
    .await;
    let (_dir, engine) = engine(Config {
        max_connections: 1,
        max_connections_per_host: 1,
        ..Config::default()
    })
    .await;
    let token = CancellationToken::new();
    let worker = {
        let engine = engine.clone();
        let url = format!("{}/stall", server.origin);
        let token = token.clone();
        tokio::spawn(async move { engine.inspect_hls(&url, &[], &token).await })
    };
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    assert!(!engine.shutdown_if_idle().await.unwrap());
    token.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::Interrupted)
    ));
    inspect(&engine, &format!("{}/ok", server.origin))
        .await
        .unwrap();
    assert!(engine.shutdown_if_idle().await.unwrap());
}

#[tokio::test]
async fn shutdown_drains_inspections_and_releases_database_ownership() {
    let entered = Arc::new(Notify::new());
    let notification = entered.clone();
    let server = server(Router::new().route(
        "/stall",
        get(move || {
            let notification = notification.clone();
            async move { stalled_body(notification) }
        }),
    ))
    .await;
    let (dir, engine) = engine(Config::default()).await;
    let worker = {
        let engine = engine.clone();
        let url = format!("{}/stall", server.origin);
        tokio::spawn(async move { inspect(&engine, &url).await })
    };
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), engine.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(worker.await.unwrap(), Err(Error::Shutdown)));
    assert!(matches!(
        inspect(&engine, &format!("{}/stall", server.origin)).await,
        Err(Error::Shutdown)
    ));
    let reopened = Engine::open(dir.path().join("media.db"), Config::default())
        .await
        .unwrap();
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_stops_retry_after_and_shared_cooldown_waits() {
    let entered = Arc::new(Notify::new());
    let notification = entered.clone();
    let server = server(
        Router::new()
            .route(
                "/limited",
                get(move || {
                    notification.notify_one();
                    async { (StatusCode::TOO_MANY_REQUESTS, [("retry-after", "3600")]) }
                }),
            )
            .route("/ok", get(|| async { MASTER })),
    )
    .await;
    let (_dir, engine) = engine(Config::default()).await;
    let token = CancellationToken::new();
    let worker = {
        let engine = engine.clone();
        let token = token.clone();
        let url = format!("{}/limited", server.origin);
        tokio::spawn(async move { engine.inspect_hls(&url, &[], &token).await })
    };
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    token.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::Interrupted)
    ));
    let token = CancellationToken::new();
    let worker = {
        let engine = engine.clone();
        let token = token.clone();
        let url = format!("{}/ok", server.origin);
        tokio::spawn(async move { engine.inspect_hls(&url, &[], &token).await })
    };
    token.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::Interrupted)
    ));
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn errors_never_include_source_or_redirect_urls() {
    let server =
        server(Router::new().route("/error", get(|| async { StatusCode::FORBIDDEN }))).await;
    let (_dir, engine) = engine(Config::default()).await;
    let error = inspect(
        &engine,
        &format!("{}/error?credential=secret-value", server.origin),
    )
    .await
    .unwrap_err();
    for diagnostic in [error.to_string(), format!("{error:?}")] {
        assert!(!diagnostic.contains("secret-value"));
        assert!(!diagnostic.contains(&server.origin));
    }
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn global_and_host_admission_waits_are_cancellable_without_sending_requests() {
    for (global, host) in [(1, 8), (8, 1)] {
        let entered = Arc::new(Notify::new());
        let notification = entered.clone();
        let hits = Arc::new(AtomicUsize::new(0));
        let count = hits.clone();
        let server = server(
            Router::new()
                .route(
                    "/stall",
                    get(move || {
                        let notification = notification.clone();
                        async move { stalled_body(notification) }
                    }),
                )
                .route(
                    "/ok",
                    get(move || {
                        count.fetch_add(1, Ordering::SeqCst);
                        async { MASTER }
                    }),
                ),
        )
        .await;
        let (_dir, engine) = engine(Config {
            max_connections: global,
            max_connections_per_host: host,
            ..Config::default()
        })
        .await;
        let blocker = {
            let engine = engine.clone();
            let url = format!("{}/stall", server.origin);
            tokio::spawn(async move {
                engine
                    .inspect_hls(&url, &[], &CancellationToken::new())
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .unwrap();
        let token = CancellationToken::new();
        let waiting = {
            let engine = engine.clone();
            let token = token.clone();
            let url = format!("{}/ok", server.origin);
            tokio::spawn(async move { engine.inspect_hls(&url, &[], &token).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        token.cancel();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), waiting)
                .await
                .unwrap()
                .unwrap(),
            Err(Error::Interrupted)
        ));
        // Dropping an API future must also release its lifecycle and connection
        // leases; there is no detached request task left to drain.
        blocker.abort();
        assert!(blocker.await.unwrap_err().is_cancelled());
        inspect(&engine, &format!("{}/ok", server.origin))
            .await
            .unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(engine.shutdown_if_idle().await.unwrap());
    }
}

#[tokio::test]
async fn bandwidth_throttle_is_shared_and_cancellable() {
    let entered = Arc::new(Notify::new());
    let notification = entered.clone();
    let server = server(Router::new().route(
        "/master",
        get(move || {
            notification.notify_one();
            async { MASTER }
        }),
    ))
    .await;
    let (_dir, engine) = engine(Config {
        bytes_per_second: 1,
        ..Config::default()
    })
    .await;
    let token = CancellationToken::new();
    let worker = {
        let engine = engine.clone();
        let token = token.clone();
        let url = format!("{}/master", server.origin);
        tokio::spawn(async move { engine.inspect_hls(&url, &[], &token).await })
    };
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!worker.is_finished());
    token.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::Interrupted)
    ));
    engine.set_speed_limit(0).await;
    inspect(&engine, &format!("{}/master", server.origin))
        .await
        .unwrap();
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn transient_failures_retry_with_a_bounded_interactive_budget() {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let server = server(Router::new().route(
        "/master",
        get(move || {
            let hit = count.fetch_add(1, Ordering::SeqCst);
            async move {
                if hit == 0 {
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                } else {
                    MASTER.into_response()
                }
            }
        }),
    ))
    .await;
    let (_dir, engine) = engine(Config {
        max_retries: 1,
        ..Config::default()
    })
    .await;
    inspect(&engine, &format!("{}/master", server.origin))
        .await
        .unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    engine.shutdown().await.unwrap();
}
