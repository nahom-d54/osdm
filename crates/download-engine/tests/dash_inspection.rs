use axum::{Router, http::StatusCode, routing::get};
use download_engine::{Config, Engine, Error, dash_track_key, select_dash_tracks};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;

const MPD: &str = r#"<MPD type="static"><Period><AdaptationSet id="video" mimeType="video/mp4"><Representation id="main" bandwidth="800000" width="640" height="360"><SegmentList duration="2"><Initialization sourceURL="init.mp4"/><SegmentURL media="one.m4s"/></SegmentList></Representation></AdaptationSet><AdaptationSet id="audio" mimeType="audio/mp4" lang="en"><Representation id="main" bandwidth="64000"><SegmentList duration="2"><Initialization sourceURL="audio-init.mp4"/><SegmentURL media="audio.m4s"/></SegmentList></Representation></AdaptationSet></Period></MPD>"#;

async fn fixture(
    router: Router,
) -> (
    String,
    tokio::task::JoinHandle<()>,
    tempfile::TempDir,
    Engine,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(
        dir.path().join("dash.db"),
        Config {
            max_retries: 0,
            ..Config::default()
        },
    )
    .await
    .unwrap();
    (origin, task, dir, engine)
}

#[tokio::test]
async fn dash_inspection_preserves_quality_and_adaptation_identity_without_fetching_tracks() {
    let (origin, server, _dir, engine) =
        fixture(Router::new().route("/manifest.mpd", get(|| async { MPD }))).await;
    let plan = engine
        .inspect_dash(
            &format!("{origin}/manifest.mpd"),
            &[],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(plan.tracks.len(), 2);
    assert_eq!(plan.tracks[0].width, Some(640));
    assert_eq!(
        plan.tracks[0].initialization.as_ref().unwrap().as_str(),
        format!("{origin}/init.mp4")
    );
    let video = dash_track_key(&plan.tracks[0]);
    let audio = dash_track_key(&plan.tracks[1]);
    assert_ne!(video, audio);
    assert_eq!(
        select_dash_tracks(&plan, &[video.clone(), audio])
            .unwrap()
            .len(),
        2
    );
    assert!(select_dash_tracks(&plan, &[]).is_err());
    assert!(select_dash_tracks(&plan, &[video.clone(), video]).is_err());
    assert!(select_dash_tracks(&plan, &["not-the-selected-representation".into()]).is_err());
    assert!(engine.snapshot().await.is_empty());
    engine.shutdown().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn dash_cross_origin_redirect_is_checked_before_sending_and_requires_explicit_approval() {
    let count = Arc::new(AtomicUsize::new(0));
    let hits = count.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new().route(
        "/manifest.mpd",
        get(move |headers: axum::http::HeaderMap| {
            hits.fetch_add(1, Ordering::SeqCst);
            assert!(
                !headers.contains_key("cookie")
                    && !headers.contains_key("authorization")
                    && !headers.contains_key("referer")
            );
            async { MPD }
        }),
    );
    let target_server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let location = format!("{target}/manifest.mpd");
    let (origin, server, _dir, engine) = fixture(Router::new().route(
        "/start",
        get(move || {
            let location = location.clone();
            async move { (StatusCode::FOUND, [("location", location)]) }
        }),
    ))
    .await;
    assert!(
        engine
            .inspect_dash(&format!("{origin}/start"), &[], &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    engine
        .inspect_dash(
            &format!("{origin}/start"),
            &[target],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    engine.shutdown().await.unwrap();
    server.abort();
    target_server.abort();
}

#[tokio::test]
async fn dash_loops_unsupported_documents_and_pre_cancelled_requests_fail_closed() {
    let (origin, server, _dir, engine) = fixture(
        Router::new()
            .route(
                "/loop",
                get(|| async { (StatusCode::FOUND, [("location", "/loop")]) }),
            )
            .route(
                "/live",
                get(|| async { "<MPD type=\"dynamic\"><Period/></MPD>" }),
            ),
    )
    .await;
    assert!(matches!(
        engine
            .inspect_dash(&format!("{origin}/loop"), &[], &CancellationToken::new())
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(
        engine
            .inspect_dash(&format!("{origin}/live"), &[], &CancellationToken::new())
            .await
            .is_err()
    );
    let token = CancellationToken::new();
    token.cancel();
    assert!(matches!(
        engine
            .inspect_dash(&format!("{origin}/loop"), &[], &token)
            .await,
        Err(Error::Interrupted)
    ));
    engine.shutdown().await.unwrap();
    server.abort();
}
