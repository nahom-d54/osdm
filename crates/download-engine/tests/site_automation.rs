use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::get,
};
use download_engine::{
    Config, DownloadState, Engine, SiteAction, SitePolicy, SiteProject, SiteProjectRequest,
    SiteProjectState,
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
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
async fn serve(router: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { origin, task }
}
fn request(origin: &str, path: &Path) -> SiteProjectRequest {
    SiteProjectRequest {
        name: "Bounded mirror".into(),
        seed_url: format!("{origin}/docs/"),
        path_prefix: "/docs/".into(),
        destination: path.into(),
        extensions: vec!["pdf".into()],
        excluded_paths: vec![],
        max_pages: 20,
        max_files: 100,
        max_bytes: 8 * 1024 * 1024,
        max_depth: 3,
        confirmed: true,
    }
}
async fn view(engine: &Engine, id: Uuid) -> SiteProject {
    engine
        .list_site_projects()
        .await
        .into_iter()
        .find(|job| job.id == id)
        .unwrap()
}
async fn ready(engine: &Engine, id: Uuid) -> SiteProject {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = view(engine, id).await;
            if job.state != SiteProjectState::Exploring {
                assert_eq!(job.state, SiteProjectState::Ready, "{:?}", job.error);
                return job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
async fn complete(engine: &Engine, id: Uuid) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let download = engine
                .snapshot()
                .await
                .into_iter()
                .find(|download| download.id == id)
                .unwrap();
            match download.state {
                DownloadState::Completed => return,
                DownloadState::Failed | DownloadState::Canceled => {
                    panic!("transfer stopped: {:?}", download.error)
                }
                _ => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .unwrap();
}
async fn basic() -> Server {
    serve(
        Router::new()
            .route(
                "/robots.txt",
                get(|| async { "User-agent: *\nDisallow:\n" }),
            )
            .route(
                "/docs/",
                get(|| async {
                    (
                        [("content-type", "text/html")],
                        "<a href='one.pdf'>PDF</a><img src='two.png'>",
                    )
                }),
            )
            .route("/docs/one.pdf", get(|| async { "one" }))
            .route("/docs/two.png", get(|| async { "two" })),
    )
    .await
}

#[tokio::test]
async fn confirmed_filter_changes_keep_files_and_restrict_the_same_scope() {
    let source = basic().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("site.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .create_site_project(request(&source.origin, dir.path()))
        .await
        .unwrap();
    let first = ready(&engine, id).await;
    assert_eq!(first.resources[0].description, "PDF");
    let child = engine
        .queue_site_resources(id, vec![first.resources[0].id.clone()])
        .await
        .unwrap()[0];
    complete(&engine, child).await;
    let original = dir.path().join(&first.resources[0].relative_path);
    let filters = site_grabber::Filters {
        extensions: ["png".into()].into_iter().collect(),
        excluded_paths: vec![],
    };
    assert!(
        engine
            .update_site_filters(id, filters.clone(), false)
            .await
            .is_err()
    );
    engine.update_site_filters(id, filters, true).await.unwrap();
    assert_eq!(view(&engine, id).await.state, SiteProjectState::Paused);
    engine
        .control_site_project(id, SiteAction::Resume)
        .await
        .unwrap();
    let second = ready(&engine, id).await;
    assert_eq!(second.generation, 1);
    assert_eq!(second.resources.len(), 1);
    assert!(second.resources[0].url.ends_with("two.png"));
    assert!(second.bytes > first.bytes);
    assert_eq!(tokio::fs::read(original).await.unwrap(), b"one");
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn periodic_slots_persist_coalesce_and_never_replay_after_clock_reversal() {
    let source = basic().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("site.db");
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    let id = engine
        .create_site_project(request(&source.origin, dir.path()))
        .await
        .unwrap();
    ready(&engine, id).await;
    let at = 4_102_444_800;
    let policy = SitePolicy {
        next_run: Some(at),
        interval_seconds: Some(60),
        ..SitePolicy::default()
    };
    assert!(
        engine
            .configure_site_policy(id, policy.clone(), false)
            .await
            .is_err()
    );
    engine
        .configure_site_policy(id, policy, true)
        .await
        .unwrap();
    engine.site_schedule_tick(at, false).await.unwrap();
    let first = ready(&engine, id).await;
    assert_eq!(first.generation, 1);
    assert_eq!(first.last_run, Some(at));
    assert_eq!(first.policy.next_run, Some(at + 60));
    engine.site_schedule_tick(at, false).await.unwrap();
    engine.site_schedule_tick(at - 1000, false).await.unwrap();
    assert_eq!(view(&engine, id).await.generation, 1);
    engine.shutdown().await.unwrap();
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    engine.site_schedule_tick(at + 180, true).await.unwrap();
    let skipped = view(&engine, id).await;
    assert_eq!(skipped.generation, 1);
    assert_eq!(skipped.policy.next_run, Some(at + 240));
    assert!(skipped.schedule_note.unwrap().contains("skipped"));
    let policy = SitePolicy {
        next_run: Some(at + 300),
        interval_seconds: Some(60),
        catch_up_on_startup: true,
        ..SitePolicy::default()
    };
    engine
        .configure_site_policy(id, policy, true)
        .await
        .unwrap();
    engine.site_schedule_tick(at + 420, true).await.unwrap();
    assert_eq!(ready(&engine, id).await.generation, 2);
    assert_eq!(view(&engine, id).await.policy.next_run, Some(at + 480));
    engine
        .control_site_project(id, SiteAction::Pause)
        .await
        .unwrap();
    assert_eq!(view(&engine, id).await.policy.next_run, None);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn explicit_automatic_mirroring_queues_only_matching_known_sizes() {
    let release = Arc::new(Notify::new());
    let entered = Arc::new(Notify::new());
    let gate = release.clone();
    let notification = entered.clone();
    let gets = Arc::new(AtomicUsize::new(0));
    let count = gets.clone();
    let source = serve(Router::new()
        .route("/robots.txt", get(|| async { "User-agent: *\nDisallow:\n" }))
        .route("/docs/", get(move || { let gate = gate.clone(); let notification = notification.clone(); async move {
            notification.notify_one(); gate.notified().await;
            ([("content-type", "text/html")], "<a href='one.pdf'>file</a><a href='unknown.pdf'>unknown</a><a href='movie.mp4'>not allowed</a>")
        }}))
        .route("/docs/one.pdf", get(move |method: Method| { let count = count.clone(); async move {
            if method == Method::HEAD { ([("content-length", "3")], Body::empty()).into_response() }
            else { count.fetch_add(1, Ordering::SeqCst); "one".into_response() }
        }}))
        .route("/docs/unknown.pdf", get(|| async { Body::from_stream(futures_util::stream::once(async { Ok::<_, std::io::Error>("no known length") })) }))).await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("site.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .create_site_project(request(&source.origin, dir.path()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    assert_eq!(gets.load(Ordering::SeqCst), 0);
    engine
        .configure_site_policy(
            id,
            SitePolicy {
                automatic_downloads: true,
                ..SitePolicy::default()
            },
            true,
        )
        .await
        .unwrap();
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = view(&engine, id).await;
            if let Some(child) = job
                .resources
                .iter()
                .find_map(|resource| resource.download_id)
            {
                complete(&engine, child).await;
                break;
            }
            assert_ne!(job.state, SiteProjectState::Failed);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let job = view(&engine, id).await;
    assert_eq!(
        job.resources
            .iter()
            .filter(|resource| resource.download_id.is_some())
            .count(),
        1
    );
    assert!(
        job.resources
            .iter()
            .any(|resource| resource.size.is_none() && resource.download_id.is_none())
    );
    assert_eq!(gets.load(Ordering::SeqCst), 1);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn rechecks_publish_separate_revisions_and_preserve_lifetime_budget_across_restart() {
    let revision = Arc::new(AtomicUsize::new(0));
    let value = revision.clone();
    let source = serve(
        Router::new()
            .route(
                "/robots.txt",
                get(|| async { "User-agent: *\nDisallow:\n" }),
            )
            .route(
                "/docs/",
                get(|| async {
                    (
                        [("content-type", "text/html")],
                        "<a href='one.pdf'>file</a>",
                    )
                }),
            )
            .route(
                "/docs/one.pdf",
                get(move || {
                    let value = value.clone();
                    async move {
                        if value.load(Ordering::SeqCst) == 0 {
                            "one"
                        } else {
                            "two"
                        }
                    }
                }),
            ),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("site.db");
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    let id = engine
        .create_site_project(request(&source.origin, dir.path()))
        .await
        .unwrap();
    let first = ready(&engine, id).await;
    let child = engine
        .queue_site_resources(id, vec![first.resources[0].id.clone()])
        .await
        .unwrap()[0];
    complete(&engine, child).await;
    let first_bytes = view(&engine, id).await.bytes;
    revision.store(1, Ordering::SeqCst);
    engine
        .control_site_project(id, SiteAction::Recheck)
        .await
        .unwrap();
    let second = ready(&engine, id).await;
    assert_ne!(
        first.resources[0].relative_path,
        second.resources[0].relative_path
    );
    assert!(
        engine
            .queue_site_resources(id, vec![first.resources[0].id.clone()])
            .await
            .is_err(),
        "stale review choices must never select a new revision"
    );
    let child = engine
        .queue_site_resources(id, vec![second.resources[0].id.clone()])
        .await
        .unwrap()[0];
    complete(&engine, child).await;
    assert_eq!(
        tokio::fs::read(dir.path().join(&first.resources[0].relative_path))
            .await
            .unwrap(),
        b"one"
    );
    assert_eq!(
        tokio::fs::read(dir.path().join(&second.resources[0].relative_path))
            .await
            .unwrap(),
        b"two"
    );
    let final_bytes = view(&engine, id).await.bytes;
    assert!(final_bytes > first_bytes);
    engine.shutdown().await.unwrap();
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    assert_eq!(view(&engine, id).await.bytes, final_bytes);
    assert_eq!(view(&engine, id).await.generation, 1);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn conditional_page_cache_revalidates_but_robots_are_always_fresh() {
    let conditional = Arc::new(AtomicUsize::new(0));
    let count = conditional.clone();
    let robots = Arc::new(AtomicUsize::new(0));
    let robot_count = robots.clone();
    let source = serve(
        Router::new()
            .route(
                "/robots.txt",
                get(move || {
                    robot_count.fetch_add(1, Ordering::SeqCst);
                    async { "User-agent: *\nDisallow:\n" }
                }),
            )
            .route(
                "/docs/",
                get(move |headers: HeaderMap| {
                    let count = count.clone();
                    async move {
                        if headers
                            .get("if-none-match")
                            .is_some_and(|value| value == "\"page-v1\"")
                        {
                            count.fetch_add(1, Ordering::SeqCst);
                            (StatusCode::NOT_MODIFIED, [("etag", "\"page-v1\"")]).into_response()
                        } else {
                            (
                                [
                                    ("content-type", "text/html"),
                                    ("etag", "\"page-v1\""),
                                    ("cache-control", "public, max-age=60"),
                                ],
                                "<a href='one.pdf'>file</a>",
                            )
                                .into_response()
                        }
                    }
                }),
            )
            .route("/docs/one.pdf", get(|| async { "one" })),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("site.db");
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    let id = engine
        .create_site_project(request(&source.origin, dir.path()))
        .await
        .unwrap();
    ready(&engine, id).await;
    engine
        .configure_site_policy(
            id,
            SitePolicy {
                cache_responses: true,
                ..SitePolicy::default()
            },
            true,
        )
        .await
        .unwrap();
    engine
        .control_site_project(id, SiteAction::Recheck)
        .await
        .unwrap();
    ready(&engine, id).await;
    engine.shutdown().await.unwrap();
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    engine
        .control_site_project(id, SiteAction::Recheck)
        .await
        .unwrap();
    assert_eq!(ready(&engine, id).await.resources.len(), 1);
    assert_eq!(conditional.load(Ordering::SeqCst), 1);
    assert_eq!(robots.load(Ordering::SeqCst), 3);
    engine
        .configure_site_policy(id, SitePolicy::default(), true)
        .await
        .unwrap();
    engine
        .control_site_project(id, SiteAction::Recheck)
        .await
        .unwrap();
    ready(&engine, id).await;
    assert_eq!(conditional.load(Ordering::SeqCst), 1);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn command_owner_does_not_dispatch_scheduled_exploration() {
    let source = basic().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("site.db");
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    let id = engine
        .create_site_project(request(&source.origin, dir.path()))
        .await
        .unwrap();
    ready(&engine, id).await;
    engine
        .configure_site_policy(
            id,
            SitePolicy {
                next_run: Some(4_102_444_800),
                ..SitePolicy::default()
            },
            true,
        )
        .await
        .unwrap();
    engine.shutdown().await.unwrap();
    let engine = Engine::open_for_commands(&db, Config::default())
        .await
        .unwrap();
    engine
        .site_schedule_tick(4_102_444_800, false)
        .await
        .unwrap();
    assert_eq!(view(&engine, id).await.generation, 0);
    assert_eq!(view(&engine, id).await.policy.next_run, Some(4_102_444_800));
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn project_concurrency_is_enforced_by_the_shared_download_scheduler() {
    for limit in [1, 2] {
        let started = Arc::new(AtomicUsize::new(0));
        let count = started.clone();
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let gate = release.clone();
        let handler = move |method: Method| {
            let count = count.clone();
            let gate = gate.clone();
            async move {
                if method == Method::HEAD {
                    return ([("content-length", "3")], Body::empty()).into_response();
                }
                count.fetch_add(1, Ordering::SeqCst);
                let body = Body::from_stream(futures_util::stream::once(async move {
                    gate.acquire_owned().await.unwrap().forget();
                    Ok::<_, std::io::Error>("one")
                }));
                ([("content-length", "3")], body).into_response()
            }
        };
        let source = serve(
            Router::new()
                .route(
                    "/robots.txt",
                    get(|| async { "User-agent: *\nDisallow:\n" }),
                )
                .route(
                    "/docs/",
                    get(|| async {
                        (
                            [("content-type", "text/html")],
                            "<a href='one.pdf'>one</a><a href='two.pdf'>two</a>",
                        )
                    }),
                )
                .route("/docs/one.pdf", get(handler.clone()))
                .route("/docs/two.pdf", get(handler)),
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path().join("site.db"), Config::default())
            .await
            .unwrap();
        let id = engine
            .create_site_project(request(&source.origin, dir.path()))
            .await
            .unwrap();
        let job = ready(&engine, id).await;
        engine
            .configure_site_policy(
                id,
                SitePolicy {
                    max_concurrent_downloads: limit,
                    ..SitePolicy::default()
                },
                true,
            )
            .await
            .unwrap();
        let children = engine
            .queue_site_resources(
                id,
                job.resources
                    .iter()
                    .map(|resource| resource.id.clone())
                    .collect(),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while started.load(Ordering::SeqCst) < limit {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(350)).await;
        assert_eq!(started.load(Ordering::SeqCst), limit);
        release.add_permits(2);
        for child in children {
            complete(&engine, child).await;
        }
        assert_eq!(started.load(Ordering::SeqCst), 2);
        engine.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn no_store_responses_are_never_conditionally_cached() {
    let conditional = Arc::new(AtomicUsize::new(0));
    let count = conditional.clone();
    let source = serve(
        Router::new()
            .route(
                "/robots.txt",
                get(|| async { "User-agent: *\nDisallow:\n" }),
            )
            .route(
                "/docs/",
                get(move |headers: HeaderMap| {
                    let count = count.clone();
                    async move {
                        if headers.contains_key("if-none-match") {
                            count.fetch_add(1, Ordering::SeqCst);
                        }
                        (
                            [
                                ("content-type", "text/html"),
                                ("etag", "\"page-v1\""),
                                ("cache-control", "no-store"),
                            ],
                            "<a href='one.pdf'>file</a>",
                        )
                    }
                }),
            )
            .route("/docs/one.pdf", get(|| async { "one" })),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("site.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .create_site_project(request(&source.origin, dir.path()))
        .await
        .unwrap();
    ready(&engine, id).await;
    engine
        .configure_site_policy(
            id,
            SitePolicy {
                cache_responses: true,
                ..SitePolicy::default()
            },
            true,
        )
        .await
        .unwrap();
    for _ in 0..2 {
        engine
            .control_site_project(id, SiteAction::Recheck)
            .await
            .unwrap();
        ready(&engine, id).await;
    }
    assert_eq!(conditional.load(Ordering::SeqCst), 0);
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn schema_eight_policy_migration_is_transactional_and_old_binaries_reject_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("upgrade.db");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&db)
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
    sqlx::query("PRAGMA user_version=7")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    engine.shutdown().await.unwrap();
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&db))
            .await
            .unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 9);
    assert!(
        !(0..=7).contains(&version),
        "revision-7 guard must refuse policy databases"
    );
    let marker: i64 = sqlx::query_scalar("SELECT version FROM site_policy_version WHERE id=1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(marker, 1);
    sqlx::query("PRAGMA user_version=10")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(Engine::open(&db, Config::default()).await.is_err());

    let bad = dir.path().join("rollback.db");
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&bad)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query("PRAGMA user_version=7")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE site_policy_version (unexpected TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(Engine::open(&bad, Config::default()).await.is_err());
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&bad))
            .await
            .unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 7);
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name='site_projects'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    pool.close().await;
}
