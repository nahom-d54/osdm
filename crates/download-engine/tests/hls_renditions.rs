use axum::{Router, routing::get};
use download_engine::{
    Config, Engine, MediaJobRequest, MediaJobState, hls_rendition_key, hls_variant_key,
    media_capture::{HlsRenditionKind, Manifest},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

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
fn master(audio: &str) -> String {
    format!(
        "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\",DEFAULT=YES,AUTOSELECT=YES,URI=\"{audio}\"\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",NAME=\"Captions\",LANGUAGE=\"en\",URI=\"subs.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=100000,AUDIO=\"a\",SUBTITLES=\"s\"\nvideo.m3u8\n"
    )
}
fn request(origin: &str, destination: PathBuf) -> MediaJobRequest {
    MediaJobRequest {
        url: format!("{origin}/master.m3u8"),
        destination,
        allowed_origins: vec![],
        ffmpeg_path: std::env::current_exe().unwrap(),
        description: None,
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
async fn external_origins_and_explicit_group_choices_are_checked_before_fetching_tracks() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let external = serve(Router::new().route(
        "/audio.m3u8",
        get(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async { "should not be contacted" }
        }),
    ))
    .await;
    let text = master(&format!("{}/audio.m3u8", external.origin));
    let source = serve(Router::new().route(
        "/master.m3u8",
        get(move || {
            let text = text.clone();
            async move { text }
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("job.db"), Config::default())
        .await
        .unwrap();
    let Manifest::Master { variants } = engine
        .inspect_hls(
            &format!("{}/master.m3u8", source.origin),
            &[],
            &CancellationToken::new(),
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    let mut req = request(&source.origin, dir.path().join("output.mkv"));
    req.hls_variant_key = Some(hls_variant_key(&variants[0]));
    assert!(
        engine
            .create_media_job(req.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("explicitly select")
    );
    req.hls_audio_key = Some("stale-group-choice".into());
    assert!(engine.create_media_job(req.clone()).await.is_err());
    req.hls_audio_key = Some(hls_rendition_key(&variants[0].renditions[0]));
    assert!(
        engine
            .create_media_job(req.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("origin")
    );
    req.hls_variant_key = Some("stale-variant".into());
    assert!(engine.create_media_job(req).await.is_err());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert!(engine.snapshot().await.is_empty());
    assert!(engine.list_media_jobs().await.is_empty());
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn real_external_audio_and_segmented_webvtt_merge_with_shared_timestamps() {
    let ffmpeg = std::env::var_os("OSDM_TEST_FFMPEG")
        .map(PathBuf::from)
        .or_else(|| {
            let path = PathBuf::from("C:/apps/ffmpeg/bin/ffmpeg.exe");
            path.is_file().then_some(path)
        });
    let Some(ffmpeg) = ffmpeg else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let video_path = dir.path().join("source-video.ts");
    let audio_path = dir.path().join("source-audio.ts");
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
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:sample_rate=48000",
            "-t",
            "2",
            "-c:v",
            "libx264",
            "-bf",
            "0",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-threads",
            "1",
            "-c:a",
            "aac",
            "-f",
            "mpegts",
            "-muxdelay",
            "0",
        ])
        .arg(&video_path)
        .status()
        .await
        .unwrap();
    assert!(status.success());
    let status = tokio::process::Command::new(&ffmpeg)
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "2",
            "-c:a",
            "aac",
            "-output_ts_offset",
            "0.25",
            "-f",
            "mpegts",
            "-muxdelay",
            "0",
        ])
        .arg(&audio_path)
        .status()
        .await
        .unwrap();
    assert!(status.success());
    let video = Arc::new(tokio::fs::read(video_path).await.unwrap());
    let audio = Arc::new(tokio::fs::read(audio_path).await.unwrap());
    let video_pts = download_engine::media_capture::mpegts_start_pts(&video, false).unwrap();
    let audio_pts = download_engine::media_capture::mpegts_start_pts(&audio, true).unwrap();
    let expected_offset = download_engine::media_capture::mpegts_clock_delta(audio_pts, video_pts)
        .unwrap() as f64
        / 90000.0;
    let first = format!(
        "WEBVTT\nX-TIMESTAMP-MAP=LOCAL:00:00.000,MPEGTS:{video_pts}\n\n00:00.500 --> 00:01.500\nSelected caption\n"
    );
    let second = first.clone();
    let source = serve(Router::new()
        .route("/master.m3u8", get(|| async { master("audio.m3u8") }))
        .route("/video.m3u8", get(|| async { "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2,\nvideo.ts\n#EXT-X-ENDLIST\n" }))
        .route("/audio.m3u8", get(|| async { "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2,\naudio.ts\n#EXT-X-ENDLIST\n" }))
        .route("/subs.m3u8", get(|| async { "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\nfirst.vtt\n#EXTINF:1,\nsecond.vtt\n#EXT-X-ENDLIST\n" }))
        .route("/video.ts", get(move || { let bytes = video.clone(); async move { bytes.as_ref().clone() } }))
        .route("/audio.ts", get(move || { let bytes = audio.clone(); async move { bytes.as_ref().clone() } }))
        .route("/first.vtt", get(move || { let body = first.clone(); async move { body } }))
        .route("/second.vtt", get(move || { let body = second.clone(); async move { body } }))).await;
    let engine = Engine::open(dir.path().join("capture.db"), Config::default())
        .await
        .unwrap();
    let Manifest::Master { variants } = engine
        .inspect_hls(
            &format!("{}/master.m3u8", source.origin),
            &[],
            &CancellationToken::new(),
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    let mut req = request(&source.origin, dir.path().join("output.mkv"));
    req.ffmpeg_path = ffmpeg.clone();
    req.hls_variant_key = Some(hls_variant_key(&variants[0]));
    req.hls_audio_key = variants[0]
        .renditions
        .iter()
        .find(|r| r.kind == HlsRenditionKind::Audio)
        .map(hls_rendition_key);
    req.hls_subtitle_key = variants[0]
        .renditions
        .iter()
        .find(|r| r.kind == HlsRenditionKind::Subtitle)
        .map(hls_rendition_key);
    let id = engine.create_media_job(req).await.unwrap();
    engine.pause_media_job(id).await.unwrap();
    engine.shutdown().await.unwrap();
    let engine = Engine::open(dir.path().join("capture.db"), Config::default())
        .await
        .unwrap();
    assert_eq!(
        engine.list_media_jobs().await[0].state,
        MediaJobState::Paused
    );
    engine.resume_media_job(id).await.unwrap();
    let job = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let job = engine
                .list_media_jobs()
                .await
                .into_iter()
                .find(|job| job.id == id)
                .unwrap();
            if matches!(
                job.state,
                MediaJobState::Completed | MediaJobState::Failed | MediaJobState::Canceled
            ) {
                break job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(job.state, MediaJobState::Completed, "{:?}", job.error);
    assert_eq!(job.children.len(), 2);
    let probe = tokio::process::Command::new(ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    }))
    .args([
        "-v",
        "error",
        "-show_entries",
        "stream=codec_type,start_time",
        "-of",
        "json",
    ])
    .arg(&job.destination)
    .output()
    .await
    .unwrap();
    assert!(probe.status.success());
    let info: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = info["streams"].as_array().unwrap();
    assert_eq!(
        streams
            .iter()
            .filter(|s| s["codec_type"] == "audio")
            .count(),
        1,
        "muxed source audio must not leak into the selected external track"
    );
    let start = |kind| {
        streams.iter().find(|s| s["codec_type"] == kind).unwrap()["start_time"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap()
    };
    assert!(
        start("video").abs() < 0.005,
        "primary video must start at its own presentation origin"
    );
    assert!(
        (start("audio") - start("video") - expected_offset).abs() < 0.05,
        "rendition presentation offset must survive remux"
    );
    let captions = tokio::process::Command::new(&ffmpeg)
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(&job.destination)
        .args(["-map", "0:s:0", "-f", "srt", "-"])
        .output()
        .await
        .unwrap();
    assert!(captions.status.success());
    let captions = String::from_utf8(captions.stdout).unwrap();
    assert_eq!(captions.matches("Selected caption").count(), 1);
    assert!(
        captions.contains("00:00:00,500 --> 00:00:01,500"),
        "{captions}"
    );
    use sha2::Digest;
    assert_eq!(
        job.sha256,
        Some(format!(
            "{:x}",
            sha2::Sha256::digest(tokio::fs::read(&job.destination).await.unwrap())
        ))
    );
    engine.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_cancels_selected_subtitle_fetch_before_any_job_or_child_is_admitted() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let signal = entered.clone();
    let source = serve(
        Router::new()
            .route("/master.m3u8", get(|| async { master("audio.m3u8") }))
            .route(
                "/video.m3u8",
                get(|| async {
                    "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2,\nvideo.ts\n#EXT-X-ENDLIST\n"
                }),
            )
            .route(
                "/subs.m3u8",
                get(|| async {
                    "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2,\nwait.vtt\n#EXT-X-ENDLIST\n"
                }),
            )
            .route(
                "/wait.vtt",
                get(move || {
                    signal.notify_one();
                    async {
                        axum::body::Body::from_stream(futures_util::stream::pending::<
                            Result<&'static str, std::io::Error>,
                        >())
                    }
                }),
            ),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("shutdown.db"), Config::default())
        .await
        .unwrap();
    let Manifest::Master { variants } = engine
        .inspect_hls(
            &format!("{}/master.m3u8", source.origin),
            &[],
            &CancellationToken::new(),
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    let mut req = request(&source.origin, dir.path().join("output.mkv"));
    req.hls_variant_key = Some(hls_variant_key(&variants[0]));
    req.hls_audio_key = Some("none".into());
    req.hls_subtitle_key = variants[0]
        .renditions
        .iter()
        .find(|rendition| rendition.kind == HlsRenditionKind::Subtitle)
        .map(hls_rendition_key);
    let worker = engine.clone();
    let task = tokio::spawn(async move { worker.create_media_job(req).await });
    tokio::time::timeout(Duration::from_secs(3), entered.notified())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), engine.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(task.await.unwrap().is_err());
    assert!(engine.snapshot().await.is_empty());
    assert!(engine.list_media_jobs().await.is_empty());
}
