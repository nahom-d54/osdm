//! Explicit opt-in codec integration: OSDM_TEST_FFMPEG must be an absolute path.
use osdm_media_capture::*;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
#[ignore = "requires explicitly configured OSDM_TEST_FFMPEG"]
async fn real_ffmpeg_remuxes_local_ts_and_attaches_validated_subtitles() {
    let ffmpeg = PathBuf::from(
        std::env::var_os("OSDM_TEST_FFMPEG").expect("configure an explicit ffmpeg path"),
    );
    assert!(ffmpeg.is_absolute());
    let dir = tempfile::tempdir().unwrap();
    let segment = dir.path().join("segment.ts");
    let status = Command::new(&ffmpeg)
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=size=16x16:rate=1",
            "-t",
            "2",
            "-c:v",
            "mpeg2video",
            "-f",
            "mpegts",
        ])
        .arg(&segment)
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let subtitles = dir.path().join("captions.srt");
    std::fs::write(
        &subtitles,
        b"1\n00:00:00,000 --> 00:00:01,000\nAttached caption\n",
    )
    .unwrap();
    for output in [OutputContainer::Mp4, OutputContainer::Matroska] {
        let plan = MergePlan {
            tracks: vec![LocalTrack {
                selection: Default::default(),
                offset_milliseconds: 0,
                files: vec![segment.clone()],
                container: SegmentContainer::MpegTs,
            }],
            subtitles: vec![LocalSubtitle {
                path: subtitles.clone(),
                format: SubtitleFormat::Srt,
                language: Some("en".into()),
            }],
            output,
            max_input_bytes: 1024 * 1024,
        };
        let artifact = remux_local(&ffmpeg, &plan, &CancellationToken::new())
            .await
            .unwrap();
        assert!(std::fs::metadata(artifact.path()).unwrap().len() > 0);
        let extracted = Command::new(&ffmpeg)
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(artifact.path())
            .args(["-map", "0:s:0", "-f", "srt", "-"])
            .output()
            .unwrap();
        assert!(extracted.status.success());
        assert!(
            String::from_utf8(extracted.stdout)
                .unwrap()
                .contains("Attached caption")
        );
        let path = artifact.path().to_owned();
        drop(artifact);
        assert!(!path.exists());
        assert!(segment.exists());
        assert!(subtitles.exists());
    }
}

#[tokio::test]
#[ignore = "requires explicitly configured OSDM_TEST_FFMPEG and OSDM_TEST_FFPROBE"]
async fn real_fragmented_mp4_separate_tracks_and_webvtt_keep_identity() {
    let ffmpeg = PathBuf::from(std::env::var_os("OSDM_TEST_FFMPEG").unwrap());
    let ffprobe = PathBuf::from(std::env::var_os("OSDM_TEST_FFPROBE").unwrap());
    assert!(ffmpeg.is_absolute() && ffprobe.is_absolute());
    let dir = tempfile::tempdir().unwrap();
    let mut tracks = Vec::new();
    for (name, input, codec) in [
        ("video", "color=size=16x16:rate=1", "mpeg4"),
        ("audio", "sine=frequency=440:sample_rate=8000", "aac"),
    ] {
        let path = dir.path().join(format!("{name}.mp4"));
        let status = Command::new(&ffmpeg)
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                input,
                "-t",
                "2",
                "-c",
                codec,
                "-movflags",
                "frag_keyframe+empty_moov+default_base_moof",
                "-f",
                "mp4",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        let bytes = std::fs::read(&path).unwrap();
        let split = bytes.windows(4).position(|w| w == b"moof").unwrap() - 4;
        let init = dir.path().join(format!("{name}-init.mp4"));
        let fragment = dir.path().join(format!("{name}-fragment.m4s"));
        std::fs::write(&init, &bytes[..split]).unwrap();
        std::fs::write(&fragment, &bytes[split..]).unwrap();
        tracks.push(LocalTrack {
            selection: Default::default(),
            offset_milliseconds: 0,
            files: vec![init, fragment],
            container: SegmentContainer::FragmentedMp4,
        });
    }
    let subtitle = dir.path().join("sub.vtt");
    std::fs::write(
        &subtitle,
        b"WEBVTT\n\nenglish\n00:00.000 --> 00:01.000\nA separate caption\n",
    )
    .unwrap();
    let plan = MergePlan {
        tracks,
        subtitles: vec![LocalSubtitle {
            path: subtitle,
            format: SubtitleFormat::WebVtt,
            language: Some("eng".into()),
        }],
        output: OutputContainer::Mp4,
        max_input_bytes: 1024 * 1024,
    };
    let artifact = remux_local(&ffmpeg, &plan, &CancellationToken::new())
        .await
        .unwrap();
    let probe = Command::new(&ffprobe)
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(artifact.path())
        .output()
        .unwrap();
    assert!(probe.status.success());
    let metadata: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = metadata["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 3);
    assert_eq!(streams[0]["codec_type"], "video");
    assert_eq!(streams[1]["codec_type"], "audio");
    assert_eq!(streams[2]["codec_type"], "subtitle");
    assert_eq!(streams[2]["codec_name"], "mov_text");
    assert_eq!(streams[2]["tags"]["language"], "eng");
    let extraction = Command::new(&ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(artifact.path())
        .args(["-map", "0:s:0", "-f", "srt", "-"])
        .output()
        .unwrap();
    assert!(extraction.status.success());
    assert!(
        String::from_utf8(extraction.stdout)
            .unwrap()
            .contains("A separate caption")
    );
}
