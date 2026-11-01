//! Integration acceptance tests. Require site_jobs registration and durable transfer
//! policy hooks from .foreman/scratch/site-integration-hooks.md; not parser-only tests.
use axum::{
    Router,
    body::Body,
    http::{Method, StatusCode},
    response::IntoResponse,
    routing::get,
};
use download_engine::{
    Config, DownloadState, Engine, SiteAction, SiteProject, SiteProjectRequest, SiteProjectState,
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

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
fn request(origin: &str, directory: &Path) -> SiteProjectRequest {
    SiteProjectRequest {
        name: "Explicit docs mirror".into(),
        seed_url: format!("{origin}/docs/"),
        path_prefix: "/docs/".into(),
        destination: directory.into(),
        extensions: vec!["pdf".into()],
        excluded_paths: vec![],
        max_pages: 10,
        max_files: 10,
        max_bytes: 2 * 1024 * 1024,
        max_depth: 4,
        confirmed: true,
    }
}
async fn project(engine: &Engine, id: Uuid) -> SiteProject {
    engine
        .list_site_projects()
        .await
        .into_iter()
        .find(|j| j.id == id)
        .unwrap()
}
async fn settled(engine: &Engine, id: Uuid) -> SiteProject {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let p = project(engine, id).await;
            if p.state != SiteProjectState::Exploring {
                return p;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
async fn child_terminal(engine: &Engine, id: Uuid) -> DownloadState {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let d = engine
                .snapshot()
                .await
                .into_iter()
                .find(|d| d.id == id)
                .unwrap();
            if matches!(
                d.state,
                DownloadState::Completed | DownloadState::Failed | DownloadState::Canceled
            ) {
                return d.state;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

async fn fixture(gets: Arc<AtomicUsize>, heads: Arc<AtomicUsize>, changed: bool) -> Server {
    server(Router::new()
        .route("/robots.txt", get(|| async { "User-agent: OSDM\nDisallow: /docs/private\n" }))
        .route("/docs/", get(|| async { ([("content-type", "text/html")],
            "<a href='./#loop'>loop</a><a href='asset.pdf?sig=one#x'>file</a><a href='asset.pdf?sig=one#y'>same</a><a href='private.pdf'>denied</a>") }))
        .route("/docs/asset.pdf", get(move |method: Method| {
            let gets = gets.clone(); let heads = heads.clone();
            async move {
                if method == Method::HEAD {
                    heads.fetch_add(1, Ordering::SeqCst);
                    ([("content-length", "3")], Body::empty()).into_response()
                } else {
                    gets.fetch_add(1, Ordering::SeqCst);
                    if changed { "changed length".into_response() } else { "pdf".into_response() }
                }
            }
        }))).await
}

#[tokio::test]
async fn robots_html_head_review_and_hard_capped_engine_download() {
    let gets = Arc::new(AtomicUsize::new(0));
    let heads = Arc::new(AtomicUsize::new(0));
    let source = fixture(gets.clone(), heads.clone(), false).await;
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path().join("site.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .create_site_project(request(&source.origin, directory.path()))
        .await
        .unwrap();
    let ready = settled(&engine, id).await;
    assert!(ready.state == SiteProjectState::Ready);
    assert_eq!(ready.pages, 1);
    assert_eq!(ready.resources.len(), 1);
    assert_eq!(ready.resources[0].size, Some(3));
    assert_eq!(heads.load(Ordering::SeqCst), 1);
    assert_eq!(gets.load(Ordering::SeqCst), 0);
    assert!(engine.snapshot().await.is_empty());
    let resource = &ready.resources[0];
    assert!(resource.url.ends_with("?sig=one"));
    assert!(resource.download_id.is_none());
    let ids = engine
        .queue_site_resources(id, vec![resource.id.clone()])
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);
    let download = engine
        .snapshot()
        .await
        .into_iter()
        .find(|d| d.id == ids[0])
        .unwrap();
    let policy = serde_json::to_value(&download).unwrap();
    assert_eq!(policy["max_bytes"], 3);
    assert_eq!(policy["deny_redirects"], true);
    assert!(child_terminal(&engine, ids[0]).await == DownloadState::Completed);
    assert_eq!(
        tokio::fs::read(directory.path().join(&resource.relative_path))
            .await
            .unwrap(),
        b"pdf"
    );
    assert!(gets.load(Ordering::SeqCst) >= 1);
    assert_eq!(
        engine
            .queue_site_resources(id, vec![resource.id.clone()])
            .await
            .unwrap(),
        ids
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn changed_resource_length_fails_without_publication() {
    let gets = Arc::new(AtomicUsize::new(0));
    let heads = Arc::new(AtomicUsize::new(0));
    let source = fixture(gets, heads, true).await;
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path().join("site.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .create_site_project(request(&source.origin, directory.path()))
        .await
        .unwrap();
    let ready = settled(&engine, id).await;
    let resource = &ready.resources[0];
    let ids = engine
        .queue_site_resources(id, vec![resource.id.clone()])
        .await
        .unwrap();
    assert!(child_terminal(&engine, ids[0]).await == DownloadState::Failed);
    assert!(!directory.path().join(&resource.relative_path).exists());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn existing_destination_is_not_overwritten_or_renamed_away() {
    let source = fixture(
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        false,
    )
    .await;
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path().join("site.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .create_site_project(request(&source.origin, directory.path()))
        .await
        .unwrap();
    let ready = settled(&engine, id).await;
    let resource = &ready.resources[0];
    let path = directory.path().join(&resource.relative_path);
    tokio::fs::write(&path, b"user file").await.unwrap();
    assert!(
        engine
            .queue_site_resources(id, vec![resource.id.clone()])
            .await
            .is_err()
    );
    assert_eq!(tokio::fs::read(path).await.unwrap(), b"user file");
    assert!(engine.snapshot().await.is_empty());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn page_and_robots_caps_are_real_not_just_parser_limits() {
    for (route, length) in [("/docs/", 512 * 1024 + 1), ("/robots.txt", 64 * 1024 + 1)] {
        let huge = "x".repeat(length);
        let router = if route == "/docs/" {
            Router::new()
                .route(
                    "/robots.txt",
                    get(|| async { "User-agent: *\nDisallow:\n" }),
                )
                .route(
                    "/docs/",
                    get(move || {
                        let huge = huge.clone();
                        async move { ([("content-type", "text/html")], huge) }
                    }),
                )
        } else {
            Router::new().route(
                "/robots.txt",
                get(move || {
                    let huge = huge.clone();
                    async move { huge }
                }),
            )
        };
        let source = server(router).await;
        let directory = tempfile::tempdir().unwrap();
        let engine = Engine::open(directory.path().join("site.db"), Config::default())
            .await
            .unwrap();
        let id = engine
            .create_site_project(request(&source.origin, directory.path()))
            .await
            .unwrap();
        let failed = settled(&engine, id).await;
        assert!(failed.state == SiteProjectState::Failed);
        assert!(failed.resources.is_empty());
        assert!(engine.snapshot().await.is_empty());
        engine.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn pause_restart_replays_frontier_and_shutdown_refuses_active_site_work() {
    let hits = Arc::new(AtomicUsize::new(0));
    let fast = Arc::new(AtomicBool::new(false));
    let count = hits.clone();
    let speed = fast.clone();
    let source = server(
        Router::new()
            .route(
                "/robots.txt",
                get(|| async { "User-agent: *\nDisallow:\n" }),
            )
            .route(
                "/docs/",
                get(move || {
                    let count = count.clone();
                    let speed = speed.clone();
                    async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        if !speed.load(Ordering::SeqCst) {
                            tokio::time::sleep(Duration::from_secs(30)).await;
                        }
                        (
                            [("content-type", "text/html")],
                            "<a href='./#loop'>loop</a>",
                        )
                    }
                }),
            ),
    )
    .await;
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("site.db");
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let id = engine
        .create_site_project(request(&source.origin, directory.path()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while hits.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!engine.shutdown_if_idle().await.unwrap());
    tokio::time::timeout(
        Duration::from_secs(3),
        engine.control_site_project(id, SiteAction::Pause),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(project(&engine, id).await.state == SiteProjectState::Paused);
    let charged = project(&engine, id).await.bytes;
    assert!(charged >= 512 * 1024);
    engine.shutdown().await.unwrap();
    drop(engine);
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    assert!(project(&engine, id).await.state == SiteProjectState::Paused);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "opening must not explore automatically"
    );
    fast.store(true, Ordering::SeqCst);
    engine
        .control_site_project(id, SiteAction::Resume)
        .await
        .unwrap();
    let ready = settled(&engine, id).await;
    assert!(ready.state == SiteProjectState::Ready);
    assert_eq!(ready.pages, 1);
    assert!(ready.bytes > charged);
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn unconfirmed_create_has_no_network_side_effects() {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let source = server(Router::new().route(
        "/robots.txt",
        get(move || {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                "User-agent: *\nDisallow:\n"
            }
        }),
    ))
    .await;
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path().join("site.db"), Config::default())
        .await
        .unwrap();
    let mut req = request(&source.origin, directory.path());
    req.confirmed = false;
    assert!(engine.create_site_project(req).await.is_err());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert!(engine.list_site_projects().await.is_empty());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn redirects_never_fetch_cross_origin_auth_or_private_targets() {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let foreign = server(Router::new().route(
        "/secret",
        get(move || {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                "secret"
            }
        }),
    ))
    .await;
    let target = format!("{}/secret", foreign.origin);
    let source = server(Router::new()
        .route("/robots.txt", get(|| async { "User-agent: *\nDisallow: /docs/private\n" }))
        .route("/docs/", get(|| async { ([("content-type", "text/html")], "<a href='redirect'>r</a><a href='auth'>a</a><a href='private.pdf'>p</a><a href='./'>loop</a>") }))
        .route("/docs/redirect", get(move || { let target = target.clone(); async move { (StatusCode::FOUND, [("location", target)]) } }))
        .route("/docs/auth", get(|| async { (StatusCode::FOUND, [("location", "http://user:secret@127.0.0.1:1/docs/")]) }))).await;
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path().join("site.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .create_site_project(request(&source.origin, directory.path()))
        .await
        .unwrap();
    let ready = settled(&engine, id).await;
    assert!(ready.state == SiteProjectState::Ready);
    assert_eq!(ready.pages, 3);
    assert!(ready.resources.is_empty());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn unknown_head_length_is_not_implicitly_zero_and_cannot_be_queued() {
    let source = server(
        Router::new()
            .route("/robots.txt", get(|| async { StatusCode::NOT_FOUND }))
            .route(
                "/docs/",
                get(|| async {
                    (
                        [("content-type", "text/html")],
                        "<a href='unknown.pdf'>file</a>",
                    )
                }),
            )
            .route(
                "/docs/unknown.pdf",
                get(|| async { StatusCode::NO_CONTENT }),
            ),
    )
    .await;
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::open(directory.path().join("site.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .create_site_project(request(&source.origin, directory.path()))
        .await
        .unwrap();
    let ready = settled(&engine, id).await;
    assert!(ready.state == SiteProjectState::Ready);
    assert_eq!(ready.resources.len(), 1);
    assert_eq!(ready.resources[0].size, None);
    assert!(
        engine
            .queue_site_resources(id, vec![ready.resources[0].id.clone()])
            .await
            .is_err()
    );
    assert!(engine.snapshot().await.is_empty());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_drains_inflight_site_request_before_database_reopen() {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let source = server(
        Router::new()
            .route("/robots.txt", get(|| async { StatusCode::NOT_FOUND }))
            .route(
                "/docs/",
                get(move || {
                    let count = count.clone();
                    async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        "not reached"
                    }
                }),
            ),
    )
    .await;
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("site.db");
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let id = engine
        .create_site_project(request(&source.origin, directory.path()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while hits.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(3), engine.shutdown())
        .await
        .unwrap()
        .unwrap();
    drop(engine);
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    assert!(project(&engine, id).await.state == SiteProjectState::Paused);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(
        engine
            .create_site_project(request(&source.origin, directory.path()))
            .await
            .is_ok()
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn selected_resource_redirect_does_not_contact_foreign_origin() {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let foreign = server(Router::new().route(
        "/secret",
        get(move || {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                "secret"
            }
        }),
    ))
    .await;
    let target = format!("{}/secret", foreign.origin);
    let source = server(
        Router::new()
            .route("/robots.txt", get(|| async { StatusCode::NOT_FOUND }))
            .route(
                "/docs/",
                get(|| async {
                    (
                        [("content-type", "text/html")],
                        "<a href='redirect.pdf'>file</a>",
                    )
                }),
            )
            .route(
                "/docs/redirect.pdf",
                get(move |method: Method| {
                    let target = target.clone();
                    async move {
                        if method == Method::HEAD {
                            ([("content-length", "3")], Body::empty()).into_response()
                        } else {
                            (StatusCode::FOUND, [("location", target)]).into_response()
                        }
                    }
                }),
            ),
    )
    .await;
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("site.db");
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let id = engine
        .create_site_project(request(&source.origin, directory.path()))
        .await
        .unwrap();
    let ready = settled(&engine, id).await;
    let r = &ready.resources[0];
    let ids = engine
        .queue_site_resources(id, vec![r.id.clone()])
        .await
        .unwrap();
    assert!(child_terminal(&engine, ids[0]).await == DownloadState::Failed);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert!(!directory.path().join(&r.relative_path).exists());
    engine.shutdown().await.unwrap();
    drop(engine);
    let engine = Engine::open(&database, Config::default()).await.unwrap();
    let child = engine
        .snapshot()
        .await
        .into_iter()
        .find(|d| d.id == ids[0])
        .unwrap();
    let policy = serde_json::to_value(child).unwrap();
    assert_eq!(policy["max_bytes"], 3);
    assert_eq!(policy["deny_redirects"], true);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn selected_payload_resumes_after_restart_with_matching_validator_and_same_reservation() {
    const SIZE: u64 = 512 * 1024;
    let ranges = Arc::new(tokio::sync::Mutex::new(Vec::<(u64, String)>::new()));
    let recorded = ranges.clone();
    let source = server(
        Router::new()
            .route("/robots.txt", get(|| async { StatusCode::NOT_FOUND }))
            .route(
                "/docs/",
                get(|| async {
                    (
                        [("content-type", "text/html")],
                        "<a href='resumable.pdf'>file</a>",
                    )
                }),
            )
            .route(
                "/docs/resumable.pdf",
                get(move |method: Method, headers: axum::http::HeaderMap| {
                    let recorded = recorded.clone();
                    async move {
                        if method == Method::HEAD {
                            return axum::http::Response::builder()
                                .status(200)
                                .header("content-length", SIZE)
                                .header("accept-ranges", "bytes")
                                .header("etag", "\"v1\"")
                                .body(Body::empty())
                                .unwrap();
                        }
                        let range = headers
                            .get("range")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.strip_prefix("bytes="))
                            .and_then(|v| v.split_once('-'));
                        let (start, end) = range
                            .map(|(start, end)| {
                                (
                                    start.parse::<u64>().unwrap(),
                                    if end.is_empty() {
                                        SIZE - 1
                                    } else {
                                        end.parse::<u64>().unwrap().min(SIZE - 1)
                                    },
                                )
                            })
                            .unwrap_or((0, SIZE - 1));
                        if start >= SIZE || end < start {
                            return StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                        }
                        let validator = headers
                            .get("if-range")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                            .to_owned();
                        recorded.lock().await.push((start, validator));
                        let mut response = axum::http::Response::builder()
                            .status(if range.is_some() { 206 } else { 200 })
                            .header("content-length", end - start + 1)
                            .header("accept-ranges", "bytes")
                            .header("etag", "\"v1\"");
                        if range.is_some() {
                            response = response
                                .header("content-range", format!("bytes {start}-{end}/{SIZE}"));
                        }
                        let chunks =
                            futures_util::stream::unfold(start, move |position| async move {
                                if position > end {
                                    return None;
                                }
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                let length = (end - position + 1).min(64 * 1024) as usize;
                                Some((
                                    Ok::<_, std::io::Error>(vec![1u8; length]),
                                    position + length as u64,
                                ))
                            });
                        response.body(Body::from_stream(chunks)).unwrap()
                    }
                }),
            ),
    )
    .await;
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("site.db");
    let config = Config {
        connections_per_download: 1,
        ..Config::default()
    };
    let engine = Engine::open(&database, config.clone()).await.unwrap();
    let id = engine
        .create_site_project(request(&source.origin, directory.path()))
        .await
        .unwrap();
    let ready = settled(&engine, id).await;
    let resource = &ready.resources[0];
    let ids = engine
        .queue_site_resources(id, vec![resource.id.clone()])
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let d = engine
                .snapshot()
                .await
                .into_iter()
                .find(|d| d.id == ids[0])
                .unwrap();
            let received: u64 = d.segments.iter().map(|s| s.downloaded).sum();
            if received > 0 && received < SIZE {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    engine.pause(ids[0]).await.unwrap();
    let child = engine
        .snapshot()
        .await
        .into_iter()
        .find(|d| d.id == ids[0])
        .unwrap();
    assert!(child.segments.iter().map(|s| s.committed).sum::<u64>() > 0);
    let accounted = project(&engine, id).await.bytes;
    engine.shutdown().await.unwrap();
    drop(engine);
    let engine = Engine::open(&database, config).await.unwrap();
    engine.resume(ids[0]).await.unwrap();
    assert!(child_terminal(&engine, ids[0]).await == DownloadState::Completed);
    assert_eq!(
        project(&engine, id).await.bytes,
        accounted,
        "retransmission is excluded from the payload reservation"
    );
    assert!(
        ranges
            .lock()
            .await
            .iter()
            .any(|(start, validator)| *start > 0 && validator == "\"v1\"")
    );
    assert_eq!(
        tokio::fs::read(directory.path().join(&resource.relative_path))
            .await
            .unwrap(),
        vec![1u8; SIZE as usize]
    );
    engine.shutdown().await.unwrap();
}
