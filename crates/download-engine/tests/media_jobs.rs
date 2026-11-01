use axum::{Router, body::Body, http::StatusCode, response::IntoResponse, routing::get};
use download_engine::{
    Config, DownloadRequest, DownloadState, Engine, MediaJobRequest, MediaJobState,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const VOD: &str =
    "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:1,\na.ts\n#EXTINF:1,\nb.ts\n#EXT-X-ENDLIST\n";

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
fn request(origin: &str, directory: &Path) -> MediaJobRequest {
    MediaJobRequest {
        url: format!("{origin}/vod"),
        destination: directory.join("output.mkv"),
        allowed_origins: vec![],
        // Invalid-media/admission tests never launch this executable.
        ffmpeg_path: std::env::current_exe().unwrap(),
        description: Some("Explicit quality".into()),
        subtitle_url: None,
        category_id: None,
        dash_tracks: vec![],
        direct_media: false,
        hls_variant_key: None,
        hls_audio_key: None,
        hls_subtitle_key: None,
    }
}

#[tokio::test]
async fn subtitle_mime_conflicts_and_malformed_extensionless_tracks_fail_before_admission() {
    let source = server(
        Router::new()
            .route(
                "/conflict.srt",
                get(|| async {
                    (
                        [("content-type", "text/vtt")],
                        "WEBVTT\n\n00:00.000 --> 00:00.800\nFixture\n",
                    )
                }),
            )
            .route(
                "/malformed",
                get(|| async { ([("content-type", "text/vtt")], "not a subtitle document") }),
            )
            .route(
                "/undeclared",
                get(|| async { "WEBVTT\n\n00:00.000 --> 00:00.800\nFixture\n" }),
            ),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("subtitles.db"), Config::default())
        .await
        .unwrap();
    for endpoint in ["conflict.srt", "malformed", "undeclared"] {
        let mut req = request(&source.origin, dir.path());
        req.direct_media = true;
        req.url = format!("{}/never-fetched.mp4", source.origin);
        req.subtitle_url = Some(format!("{}/{endpoint}", source.origin));
        assert!(engine.create_media_job(req).await.is_err());
        assert!(engine.list_media_jobs().await.is_empty());
        assert!(engine.snapshot().await.is_empty());
        assert!(!dir.path().join("output.mkv").exists());
    }
    engine.shutdown().await.unwrap();
}
async fn terminal(engine: &Engine, id: uuid::Uuid) -> download_engine::MediaJob {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let job = engine
                .list_media_jobs()
                .await
                .into_iter()
                .find(|j| j.id == id)
                .unwrap();
            if matches!(
                job.state,
                MediaJobState::Completed | MediaJobState::Failed | MediaJobState::Canceled
            ) {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}
async fn admitted_child(engine: &Engine) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !engine.snapshot().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn requires_explicit_quality_and_approved_segment_origin_without_fetching_it() {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let foreign = server(Router::new().route(
        "/x.ts",
        get(move || {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                "secret"
            }
        }),
    ))
    .await;
    let vod = format!(
        "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\n{}/x.ts\n#EXT-X-ENDLIST\n",
        foreign.origin
    );
    let source = server(
        Router::new()
            .route(
                "/vod",
                get(move || {
                    let vod = vod.clone();
                    async move { vod }
                }),
            )
            .route(
                "/master",
                get(|| async { "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nvod\n" }),
            ),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("jobs.db"), Config::default())
        .await
        .unwrap();
    let mut req = request(&source.origin, dir.path());
    assert!(
        engine
            .create_media_job(req.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("approval")
    );
    req.url = format!("{}/master", source.origin);
    assert!(
        engine
            .create_media_job(req)
            .await
            .unwrap_err()
            .to_string()
            .contains("quality")
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert!(engine.list_media_jobs().await.is_empty());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn arbitrary_segments_are_not_concatenated_or_published() {
    let source = server(
        Router::new()
            .route("/vod", get(|| async { VOD }))
            .route("/a.ts", get(|| async { "not a container" }))
            .route("/b.ts", get(|| async { "not a container" })),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("jobs.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .create_media_job(request(&source.origin, dir.path()))
        .await
        .unwrap();
    let job = terminal(&engine, id).await;
    assert_eq!(job.state, MediaJobState::Failed);
    assert!(job.error.unwrap().contains("MPEG-TS"));
    assert!(!job.destination.exists());
    assert!(job.sha256.is_none());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn segment_redirect_never_contacts_an_unapproved_origin() {
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
    let location = format!("{}/secret", foreign.origin);
    let source = server(Router::new().route("/vod", get(|| async { VOD })).route(
        "/a.ts",
        get(move || {
            let location = location.clone();
            async move { (StatusCode::FOUND, [("location", location)]) }
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("jobs.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .create_media_job(request(&source.origin, dir.path()))
        .await
        .unwrap();
    assert_eq!(terminal(&engine, id).await.state, MediaJobState::Failed);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    engine.shutdown().await.unwrap();
}

async fn blocked_server() -> Server {
    server(Router::new().route("/vod", get(|| async { VOD })).route(
        "/a.ts",
        get(|| async {
            let body = Body::from_stream(futures_util::stream::once(async {
                std::future::pending::<()>().await;
                Ok::<_, std::convert::Infallible>(vec![0u8; 188])
            }));
            ([("content-length", "188")], body).into_response()
        }),
    ))
    .await
}

#[tokio::test]
async fn pause_restart_and_cancel_keep_durable_children_and_drain_transfers() {
    let source = blocked_server().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("jobs.db");
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    let id = engine
        .create_media_job(request(&source.origin, dir.path()))
        .await
        .unwrap();
    admitted_child(&engine).await;
    assert!(!engine.shutdown_if_idle().await.unwrap());
    tokio::time::timeout(Duration::from_secs(3), engine.pause_media_job(id))
        .await
        .unwrap()
        .unwrap();
    let children = engine.list_media_jobs().await[0].children.clone();
    assert!(!children.is_empty());
    assert!(engine.resume(children[0]).await.is_err());
    assert_eq!(
        engine.list_media_jobs().await[0].state,
        MediaJobState::Paused
    );
    engine.resume_media_job(id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), engine.shutdown())
        .await
        .unwrap()
        .unwrap();
    drop(engine);
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    assert_eq!(
        engine.list_media_jobs().await[0].state,
        MediaJobState::Paused
    );
    assert_eq!(engine.list_media_jobs().await[0].children, children);
    assert!(
        engine
            .snapshot()
            .await
            .iter()
            .all(|d| d.state == DownloadState::Paused)
    );
    engine.resume_media_job(id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), engine.cancel_media_job(id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        engine.list_media_jobs().await[0].state,
        MediaJobState::Canceled
    );
    assert!(
        engine
            .snapshot()
            .await
            .iter()
            .all(|d| matches!(d.state, DownloadState::Canceled | DownloadState::Paused))
    );
    assert!(!dir.path().join("output.mkv").exists());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn destination_reservation_is_atomic_across_media_and_ordinary_downloads() {
    let source = blocked_server().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("jobs.db"), Config::default())
        .await
        .unwrap();
    let req = request(&source.origin, dir.path());
    let (left, right) = tokio::join!(
        engine.create_media_job(req.clone()),
        engine.create_media_job(req.clone())
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert!(
        engine
            .enqueue_paused(DownloadRequest {
                url: format!("{}/a.ts", source.origin),
                destination: req.destination,
                expected_sha256: None
            })
            .await
            .is_err()
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn recovery_adopts_a_durable_paused_child_whose_parent_checkpoint_was_interrupted() {
    let source = blocked_server().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("jobs.db");
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    let id = engine
        .create_media_job(request(&source.origin, dir.path()))
        .await
        .unwrap();
    admitted_child(&engine).await;
    engine.pause_media_job(id).await.unwrap();
    let original_child = engine.list_media_jobs().await[0].children[0];
    engine.shutdown().await.unwrap();
    drop(engine);
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&db))
            .await
            .unwrap();
    let payload: String = sqlx::query_scalar("SELECT payload FROM media_jobs WHERE id=?")
        .bind(id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    payload["view"]["children"] = serde_json::json!([]);
    sqlx::query("UPDATE media_jobs SET payload=? WHERE id=?")
        .bind(payload.to_string())
        .bind(id.to_string())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    assert!(engine.resume(original_child).await.is_err());
    engine.resume_media_job(id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while engine.list_media_jobs().await[0].children.is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        engine.list_media_jobs().await[0].children,
        vec![original_child]
    );
    assert_eq!(engine.snapshot().await.len(), 1);
    engine.cancel_media_job(id).await.unwrap();
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn corrupted_working_directory_marker_is_rejected_without_deleting_files() {
    let source = blocked_server().await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("jobs.db"), Config::default())
        .await
        .unwrap();
    let id = engine
        .create_media_job(request(&source.origin, dir.path()))
        .await
        .unwrap();
    admitted_child(&engine).await;
    engine.pause_media_job(id).await.unwrap();
    let marker = dir.path().join(format!(".osdm-media-{id}")).join("owner");
    tokio::fs::write(&marker, "user file").await.unwrap();
    engine.resume_media_job(id).await.unwrap();
    assert_eq!(terminal(&engine, id).await.state, MediaJobState::Failed);
    assert_eq!(tokio::fs::read(&marker).await.unwrap(), b"user file");
    engine.shutdown().await.unwrap();
}

fn ffmpeg() -> Option<PathBuf> {
    std::env::var_os("OSDM_TEST_FFMPEG")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|p| {
                    p.join(if cfg!(windows) {
                        "ffmpeg.exe"
                    } else {
                        "ffmpeg"
                    })
                })
                .find(|p| p.is_file())
        })
}

#[tokio::test]
async fn real_ffmpeg_remuxes_complete_vod_and_never_overwrites_a_racing_output() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("requires installed ffmpeg or OSDM_TEST_FFMPEG");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let fixture = dir.path().join("fixture.ts");
    let result = tokio::process::Command::new(&ffmpeg)
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x64:rate=10",
            "-t",
            "1",
            "-c:v",
            "mpeg2video",
            "-threads",
            "1",
            "-f",
            "mpegts",
        ])
        .arg(&fixture)
        .status()
        .await
        .unwrap();
    assert!(result.success());
    let bytes = Arc::new(tokio::fs::read(&fixture).await.unwrap());
    let a = bytes.clone();
    let b = bytes.clone();
    let source = server(
        Router::new()
            .route("/vod", get(|| async { VOD }))
            .route(
                "/captions.srt",
                get(|| async { "1\n00:00:00,000 --> 00:00:00,800\nTiny subtitle\n" }),
            )
            .route(
                "/a.ts",
                get(move || {
                    let bytes = a.clone();
                    async move { bytes.as_ref().clone() }
                }),
            )
            .route(
                "/b.ts",
                get(move || {
                    let bytes = b.clone();
                    async move { bytes.as_ref().clone() }
                }),
            ),
    )
    .await;
    let db = dir.path().join("jobs.db");
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    let mut req = request(&source.origin, dir.path());
    req.ffmpeg_path = ffmpeg.clone();
    req.subtitle_url = Some(format!("{}/captions.srt", source.origin));
    req.category_id = Some("video".into());
    let id = engine.create_media_job(req.clone()).await.unwrap();
    let job = terminal(&engine, id).await;
    assert_eq!(job.state, MediaJobState::Completed, "{:?}", job.error);
    assert_eq!(job.children.len(), 2);
    assert_eq!(job.completed_segments, 2);
    assert_eq!(job.sha256.as_ref().unwrap().len(), 64);
    assert_eq!(job.category_id.as_deref(), Some("video"));
    let output = tokio::fs::read(&job.destination).await.unwrap();
    assert_eq!(&output[..4], &[0x1a, 0x45, 0xdf, 0xa3]); // Matroska EBML, not TS concatenation.
    let ffprobe = ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    if ffprobe.is_file() {
        let probe = tokio::process::Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "s",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&job.destination)
            .output()
            .await
            .unwrap();
        assert!(probe.status.success());
        assert!(String::from_utf8_lossy(&probe.stdout).contains("subrip"));
    }
    req.destination = dir.path().join("racing.mkv");
    let race = engine.create_media_job(req.clone()).await.unwrap();
    tokio::fs::write(&req.destination, "user content")
        .await
        .unwrap();
    let failed = terminal(&engine, race).await;
    assert_eq!(failed.state, MediaJobState::Failed);
    assert_eq!(
        tokio::fs::read(&req.destination).await.unwrap(),
        b"user content"
    );
    engine.shutdown().await.unwrap();
    drop(engine);
    // Simulate a crash after no-clobber linking but before completion persisted.
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&db))
            .await
            .unwrap();
    let payload: String = sqlx::query_scalar("SELECT payload FROM media_jobs WHERE id=?")
        .bind(id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["ready"], true);
    payload["view"]["state"] = serde_json::json!("Publishing");
    sqlx::query("UPDATE media_jobs SET payload=? WHERE id=?")
        .bind(payload.to_string())
        .bind(id.to_string())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    assert_eq!(
        engine.list_media_jobs().await[0].state,
        MediaJobState::Paused
    );
    engine.resume_media_job(id).await.unwrap();
    assert_eq!(terminal(&engine, id).await.state, MediaJobState::Completed);
    assert_eq!(tokio::fs::read(&job.destination).await.unwrap(), output);
    engine.resume_media_job(race).await.unwrap();
    assert_eq!(terminal(&engine, race).await.state, MediaJobState::Failed);
    assert_eq!(
        tokio::fs::read(&req.destination).await.unwrap(),
        b"user content"
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn real_ffmpeg_dash_job_merges_only_explicitly_selected_tracks() {
    let Some(ffmpeg) = ffmpeg() else {
        eprintln!("requires installed ffmpeg or OSDM_TEST_FFMPEG");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let assets = dir.path().join("dash");
    tokio::fs::create_dir(&assets).await.unwrap();
    let generated = tokio::process::Command::new(&ffmpeg)
        .current_dir(&assets)
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x64:rate=10",
            "-t",
            "2",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-threads",
            "1",
            "-f",
            "dash",
            "-seg_duration",
            "1",
            "-use_template",
            "0",
            "-use_timeline",
            "0",
            "-init_seg_name",
            "init-$RepresentationID$.m4s",
            "-media_seg_name",
            "chunk-$RepresentationID$-$Number%05d$.m4s",
            "out.mpd",
        ])
        .status()
        .await
        .unwrap();
    assert!(generated.success());
    let mut init = None;
    let mut chunks = Vec::new();
    let mut entries = tokio::fs::read_dir(&assets).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("init-") {
            init = Some((name, tokio::fs::read(entry.path()).await.unwrap()));
        } else if name.starts_with("chunk-") {
            chunks.push((name, tokio::fs::read(entry.path()).await.unwrap()));
        }
    }
    chunks.sort_by(|a, b| a.0.cmp(&b.0));
    let (init_name, init_bytes) = init.expect("dash initialization segment");
    assert!(!chunks.is_empty(), "dash media segments");
    let mut mpd = format!(
        "<MPD type=\"static\"><Period><AdaptationSet id=\"video\" mimeType=\"video/mp4\"><Representation id=\"v\" bandwidth=\"100000\" width=\"64\" height=\"64\"><SegmentList duration=\"1\"><Initialization sourceURL=\"{init_name}\"/>"
    );
    for (name, _) in &chunks {
        mpd.push_str(&format!("<SegmentURL media=\"{name}\"/>"));
    }
    mpd.push_str("</SegmentList></Representation></AdaptationSet></Period></MPD>");
    let mut router = Router::new().route("/vod.mpd", get(move || async move { mpd }));
    for (name, bytes) in std::iter::once((init_name, init_bytes)).chain(chunks) {
        let bytes = Arc::new(bytes);
        router = router.route(
            &format!("/{name}"),
            get(move || {
                let bytes = bytes.clone();
                async move { bytes.as_ref().clone() }
            }),
        );
    }
    let source = server(router).await;
    let engine = Engine::open(dir.path().join("dash.db"), Config::default())
        .await
        .unwrap();
    let plan = engine
        .inspect_dash(
            &format!("{}/vod.mpd", source.origin),
            &[],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(plan.tracks.len(), 1);
    let key = download_engine::dash_track_key(&plan.tracks[0]);
    let mut req = request(&source.origin, dir.path());
    req.url = format!("{}/vod.mpd", source.origin);
    req.ffmpeg_path = ffmpeg.clone();
    req.dash_tracks = vec![key];
    let id = engine.create_media_job(req).await.unwrap();
    let job = terminal(&engine, id).await;
    assert_eq!(job.state, MediaJobState::Completed, "{:?}", job.error);
    let output = tokio::fs::read(&job.destination).await.unwrap();
    assert_eq!(&output[..4], &[0x1a, 0x45, 0xdf, 0xa3]);
    let ffprobe = ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    if ffprobe.is_file() {
        let probe = tokio::process::Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "v",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&job.destination)
            .output()
            .await
            .unwrap();
        assert!(probe.status.success());
        assert!(String::from_utf8_lossy(&probe.stdout).contains("h264"));
    }
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn direct_mp4_can_attach_webvtt_and_publish_verified_mp4() {
    let Some(ffmpeg) = ffmpeg() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.mp4");
    let status = tokio::process::Command::new(&ffmpeg)
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x64:rate=10",
            "-t",
            "1",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-threads",
            "1",
            "-movflags",
            "+faststart",
        ])
        .arg(&input)
        .status()
        .await
        .unwrap();
    assert!(status.success());
    let bytes = Arc::new(tokio::fs::read(&input).await.unwrap());
    let source = server(
        Router::new()
            .route(
                "/direct.mp4",
                get(move || {
                    let bytes = bytes.clone();
                    async move { bytes.as_ref().clone() }
                }),
            )
            .route(
                "/captions",
                get(|| async {
                    (
                        [("content-type", "text/vtt")],
                        "WEBVTT\n\n00:00.000 --> 00:00.800\nCaption fixture\n",
                    )
                }),
            ),
    )
    .await;
    let engine = Engine::open(dir.path().join("direct.db"), Config::default())
        .await
        .unwrap();
    let mut req = request(&source.origin, dir.path());
    req.url = format!("{}/direct.mp4", source.origin);
    req.subtitle_url = Some(format!("{}/captions", source.origin));
    req.destination = dir.path().join("captions.mp4");
    req.ffmpeg_path = ffmpeg.clone();
    req.direct_media = true;
    let id = engine.create_media_job(req).await.unwrap();
    let job = terminal(&engine, id).await;
    assert_eq!(job.state, MediaJobState::Completed, "{:?}", job.error);
    assert_eq!(job.children.len(), 1);
    let output = tokio::fs::read(&job.destination).await.unwrap();
    assert_eq!(&output[4..8], b"ftyp");
    use sha2::Digest;
    assert_eq!(
        job.sha256.as_deref(),
        Some(format!("{:x}", sha2::Sha256::digest(&output)).as_str())
    );
    let ffprobe = ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    if ffprobe.is_file() {
        let probe = tokio::process::Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "s",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&job.destination)
            .output()
            .await
            .unwrap();
        assert!(probe.status.success());
        assert!(String::from_utf8_lossy(&probe.stdout).contains("mov_text"));
    }
    assert!(engine.remove_completed_media_job(id, false).await.is_err());
    let input_path = engine
        .snapshot()
        .await
        .into_iter()
        .find(|download| job.children.contains(&download.id))
        .unwrap()
        .destination;
    let original_input = tokio::fs::read(&input_path).await.unwrap();
    tokio::fs::write(&input_path, b"modified media input")
        .await
        .unwrap();
    assert!(engine.remove_completed_media_job(id, true).await.is_err());
    assert_eq!(
        tokio::fs::read(&input_path).await.unwrap(),
        b"modified media input"
    );
    tokio::fs::write(&input_path, &original_input)
        .await
        .unwrap();
    let workspace = dir.path().join(format!(".osdm-media-{id}"));
    let foreign = workspace.join("user-notes.txt");
    tokio::fs::write(&foreign, b"user work").await.unwrap();
    assert!(engine.remove_completed_media_job(id, true).await.is_err());
    assert_eq!(tokio::fs::read(&foreign).await.unwrap(), b"user work");
    assert_eq!(tokio::fs::read(&job.destination).await.unwrap(), output);
    assert!(engine.list_media_jobs().await[0].removal_pending);
    engine.shutdown().await.unwrap();
    let engine = Engine::open(dir.path().join("direct.db"), Config::default())
        .await
        .unwrap();
    assert!(engine.list_media_jobs().await[0].removal_pending);
    assert!(engine.resume_media_job(id).await.is_err());
    // Remove only the synthetic file this test created, never user data.
    tokio::fs::remove_file(&foreign).await.unwrap();
    // Simulate a crash after an owned input was unlinked but before its history
    // row was removed: cleanup must remain retryable and never touch output.
    tokio::fs::remove_file(&input_path).await.unwrap();
    engine.remove_completed_media_job(id, true).await.unwrap();
    assert!(engine.list_media_jobs().await.is_empty());
    assert!(engine.snapshot().await.is_empty());
    assert_eq!(tokio::fs::read(&job.destination).await.unwrap(), output);
    let mut entries = tokio::fs::read_dir(workspace).await.unwrap();
    assert_eq!(
        entries.next_entry().await.unwrap().unwrap().file_name(),
        "owner"
    );
    assert!(entries.next_entry().await.unwrap().is_none());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn dash_job_rejects_unapproved_or_unknown_representations_before_any_download() {
    let source = server(Router::new().route(
        "/vod.mpd",
        get(|| async {
            "<MPD type=\"static\"><Period><AdaptationSet id=\"video\" mimeType=\"video/mp4\"><Representation id=\"v\" bandwidth=\"1\"><SegmentList duration=\"1\"><Initialization sourceURL=\"init.m4s\"/><SegmentURL media=\"one.m4s\"/></SegmentList></Representation></AdaptationSet></Period></MPD>"
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("dash.db"), Config::default())
        .await
        .unwrap();
    let mut req = request(&source.origin, dir.path());
    req.url = format!("{}/vod.mpd", source.origin);
    req.dash_tracks = vec!["not-a-selected-representation".into()];
    assert!(engine.create_media_job(req.clone()).await.is_err());
    assert!(engine.list_media_jobs().await.is_empty());
    assert!(engine.snapshot().await.is_empty());
    // The default job request never inspects the DASH manifest when a key is
    // supplied for a source whose resources need another origin.
    req.dash_tracks = vec![];
    req.url = format!("{}/vod", source.origin);
    assert!(engine.create_media_job(req).await.is_err());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn bounded_site_fetch_is_single_response_cancellable_and_shutdown_aware() {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let source = server(
        Router::new()
            .route(
                "/redirect",
                get(|| async { (StatusCode::FOUND, [("location", "/target")], "go") }),
            )
            .route(
                "/target",
                get(move || {
                    let count = count.clone();
                    async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        "hello"
                    }
                }),
            )
            .route("/large", get(|| async { "x".repeat(2048) }))
            .route(
                "/metadata",
                get(|| async { ([("content-length", "10485760")], Body::empty()) }),
            )
            .route(
                "/wait",
                get(|| async { std::future::pending::<String>().await }),
            ),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("jobs.db"), Config::default())
        .await
        .unwrap();
    let token = CancellationToken::new();
    let response = engine
        .fetch_site_page(&format!("{}/redirect", source.origin), 10, &token)
        .await
        .unwrap();
    assert_eq!(response.status, 302);
    assert_eq!(response.location.as_deref(), Some("/target"));
    assert_eq!(response.body, b"go");
    assert_eq!(response.received_bytes, 2);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    let metadata = engine
        .head_site_page(&format!("{}/metadata", source.origin), &token)
        .await
        .unwrap();
    assert_eq!(metadata.content_length, Some(10485760));
    assert!(metadata.body.is_empty());
    assert_eq!(metadata.received_bytes, 0);
    let redirect = engine
        .head_site_page(&format!("{}/redirect", source.origin), &token)
        .await
        .unwrap();
    assert_eq!(redirect.status, 302);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert!(
        engine
            .fetch_site_page(&format!("{}/large", source.origin), 32, &token)
            .await
            .is_err()
    );
    assert!(
        engine
            .fetch_site_page(&format!("{}/large", source.origin), 1024 * 1024 + 1, &token)
            .await
            .is_err()
    );
    let url = format!("{}/wait", source.origin);
    let operation = engine.fetch_site_page(&url, 32, &token);
    tokio::pin!(operation);
    tokio::select! { _ = &mut operation => panic!("must wait"), _ = tokio::time::sleep(Duration::from_millis(20)) => {} }
    token.cancel();
    assert!(matches!(
        operation.await,
        Err(download_engine::Error::Interrupted)
    ));
    let token = CancellationToken::new();
    let operation = engine.fetch_site_page(&url, 32, &token);
    tokio::pin!(operation);
    tokio::select! { _ = &mut operation => panic!("must wait"), _ = tokio::time::sleep(Duration::from_millis(20)) => {} }
    assert!(!engine.shutdown_if_idle().await.unwrap());
    let (response, shutdown) = tokio::join!(operation, engine.shutdown());
    assert!(matches!(response, Err(download_engine::Error::Shutdown)));
    shutdown.unwrap();
}

#[tokio::test]
async fn revision_five_upgrade_adds_media_ledger_and_rejects_old_admission_guard() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("legacy.db");
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query("PRAGMA user_version=5")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let engine = Engine::open(&db, Config::default()).await.unwrap();
    assert!(engine.list_media_jobs().await.is_empty());
    engine.shutdown().await.unwrap();
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&db))
            .await
            .unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 10);
    assert!(!(0..=5).contains(&version));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media_jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::query("PRAGMA user_version=11")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(Engine::open(&db, Config::default()).await.is_err());
}
