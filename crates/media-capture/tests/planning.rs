use osdm_media_capture::*;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

const BASE: &str = "https://media.example.test/vod/index.mpd";
const DASH: &str = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static"><Period id="p"><AdaptationSet id="video" mimeType="video/mp4" codecs="avc1.4d401e"><Representation id="720p" bandwidth="2400000" width="1280" height="720"><BaseURL>v/</BaseURL><SegmentList timescale="1000" duration="2000" startNumber="42"><Initialization sourceURL="init.mp4"/><SegmentURL media="1.m4s"/><SegmentURL media="2.m4s"/></SegmentList></Representation></AdaptationSet><AdaptationSet id="captions" mimeType="text/vtt" lang="en"><Representation id="en" bandwidth="1000"><SegmentList duration="4"><SegmentURL media="captions.vtt"/></SegmentList></Representation></AdaptationSet></Period></MPD>"#;

#[test]
fn dash_preserves_all_choices_tracks_initialization_and_identity() {
    let plan = inspect_dash(BASE, DASH.as_bytes()).unwrap();
    assert_eq!(plan.tracks.len(), 2);
    let video = &plan.tracks[0];
    assert_eq!(video.id, "720p");
    assert_eq!(video.bandwidth, 2_400_000);
    assert_eq!(video.width, Some(1280));
    assert_eq!(video.segments[0].number, 42);
    assert_eq!(video.segments[1].number, 43);
    assert_eq!(
        video.initialization.as_ref().unwrap().as_str(),
        "https://media.example.test/vod/v/init.mp4"
    );
    assert_eq!(
        video.segments[1].source.as_str(),
        "https://media.example.test/vod/v/2.m4s"
    );
    assert_eq!(plan.tracks[1].language.as_deref(), Some("en"));
    assert_eq!(plan.tracks[1].mime_type, "text/vtt");
    let encoded = serde_json::to_string(&plan).unwrap();
    assert_eq!(serde_json::from_str::<DashPlan>(&encoded).unwrap(), plan);
    assert!(!format!("{plan:?}").contains("example.test"));
}

#[test]
fn dash_baseurl_entities_and_inherited_segment_list() {
    let body = r#"<MPD type="static"><BaseURL>assets/</BaseURL><Period><AdaptationSet mimeType="audio/mp4" lang="fr"><SegmentList duration="3"><SegmentURL media="a.m4s?x=1&amp;y=2"/></SegmentList><Representation id="a" bandwidth="1"/></AdaptationSet></Period></MPD>"#;
    let plan = inspect_dash(BASE, body.as_bytes()).unwrap();
    assert_eq!(
        plan.tracks[0].segments[0].source.as_str(),
        "https://media.example.test/vod/assets/a.m4s?x=1&y=2"
    );
}

#[test]
fn dash_declared_duration_must_cover_exactly_the_explicit_segment_list() {
    let valid = DASH.replace(
        "type=\"static\"",
        "type=\"static\" mediaPresentationDuration=\"PT0H0M3.500S\"",
    );
    assert_eq!(
        inspect_dash(BASE, valid.as_bytes())
            .unwrap()
            .duration_milliseconds,
        Some(3500)
    );
    for value in [
        "PT0S",
        "PT2S",
        "PT5S",
        "P1D",
        "PTNaNS",
        "PT0.0001S",
        "PT18446744073709551615H",
    ] {
        let body = DASH.replace(
            "type=\"static\"",
            &format!("type=\"static\" mediaPresentationDuration=\"{value}\""),
        );
        assert!(inspect_dash(BASE, body.as_bytes()).is_err());
    }
    let mismatch = valid.replace("<Period id=\"p\">", "<Period id=\"p\" duration=\"PT4S\">");
    assert!(inspect_dash(BASE, mismatch.as_bytes()).is_err());
}

#[test]
fn untrusted_manifest_identity_metadata_is_not_debug_output() {
    let body = DASH.replace("720p", "https://private.example.test/?secret=hidden");
    let plan = inspect_dash(BASE, body.as_bytes()).unwrap();
    assert!(!format!("{plan:?}").contains("secret"));
    assert!(!format!("{plan:?}").contains("private.example"));
    let unsupported = DASH.replace("text/vtt", "application/ttml+xml");
    assert_eq!(
        inspect_dash(BASE, unsupported.as_bytes()),
        Err(Error::Unsupported)
    );
}

#[test]
fn dash_fails_closed_for_unsupported_semantics_and_unsafe_urls() {
    for body in [
        DASH.replace("type=\"static\"", "type=\"dynamic\""),
        DASH.replace("<Period id=\"p\">", "<Period id=\"p\"><ContentProtection/>"),
        DASH.replace(
            "<SegmentURL media=\"1.m4s\"/>",
            "<SegmentURL media=\"1.m4s\" mediaRange=\"0-99\"/>",
        ),
        DASH.replace("<SegmentURL media=\"1.m4s\"/>", "<SegmentTimeline/>"),
        DASH.replace(
            "<BaseURL>v/</BaseURL>",
            "<BaseURL>v/</BaseURL><BaseURL>other/</BaseURL>",
        ),
        DASH.replace(
            "<BaseURL>v/</BaseURL>",
            "<BaseURL>http://media.example.test/</BaseURL>",
        ),
        DASH.replace(
            "<BaseURL>v/</BaseURL>",
            "<BaseURL>file:///private/</BaseURL>",
        ),
        DASH.replace(
            "<BaseURL>v/</BaseURL>",
            "<BaseURL>https://u:p@media.example.test/</BaseURL>",
        ),
        DASH.replace(
            "<SegmentURL media=\"1.m4s\"/>",
            "<SegmentURL media=\"a#fragment\"/>",
        ),
        DASH.replace("startNumber=\"42\"", "startNumber=\"18446744073709551615\""),
        DASH.replace("<Period id=\"p\">", "<Period id=\"p\" start=\"PT5S\">"),
        DASH.replace("</Period>", "</Period><Period/>"),
        DASH.replace(
            "</Representation>",
            "<SegmentTemplate media=\"$Number$\"/></Representation>",
        ),
        format!("<!DOCTYPE MPD [<!ENTITY boom 'x'>]>{DASH}"),
        DASH.replace("</MPD>", "</Period>"),
    ] {
        assert!(
            inspect_dash(BASE, body.as_bytes()).is_err(),
            "unsupported document accepted"
        );
    }
    assert_eq!(
        inspect_dash(BASE, &vec![b' '; MAX_MANIFEST_BYTES + 1]),
        Err(Error::Limit)
    );
    let body = format!(
        "<MPD type=\"static\"><Period><AdaptationSet mimeType=\"video/mp4\"><Representation id=\"v\" bandwidth=\"1\"><SegmentList duration=\"1\">{}</SegmentList></Representation></AdaptationSet></Period></MPD>",
        "<SegmentURL media=\"a\"/>".repeat(MAX_ENTRIES + 1)
    );
    assert_eq!(inspect_dash(BASE, body.as_bytes()), Err(Error::Limit));
}

#[test]
fn url_deserialization_validates_and_redacts_errors() {
    for url in [
        "file:///private",
        "https://u:secret@example.test/",
        "https://example.test/#secret",
        " https://example.test/",
    ] {
        let error =
            serde_json::from_str::<MediaUrl>(&serde_json::to_string(url).unwrap()).unwrap_err();
        assert!(!error.to_string().contains("secret"));
        assert!(!error.to_string().contains("example.test"));
    }
}

#[test]
fn hls_quality_metadata_and_complete_vod_identity() {
    let Manifest::Master { variants } = inspect_hls(BASE, b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=2000,AVERAGE-BANDWIDTH=1500,FRAME-RATE=29.970\nv.m3u8\n").unwrap() else { panic!() };
    assert_eq!(variants[0].average_bandwidth, Some(1500));
    assert_eq!(variants[0].frame_rate.as_deref(), Some("29.970"));
    let plan = inspect_hls_vod(BASE, b"#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:42\n#EXTINF:2,\na.ts\n#EXT-X-ENDLIST\n").unwrap();
    assert_eq!(plan.media_sequence, 42);
    assert_eq!(plan.segments.len(), 1);
}

#[test]
fn subtitle_formats_timing_identity_and_bounds() {
    let srt = b"1\r\n00:00:00,000 --> 00:00:01,500\r\nHello\r\n\r\n2\r\n00:00:01,400 --> 00:00:02,000\r\nOverlap is valid\r\n";
    let info = validate_subtitles(SubtitleFormat::Srt, srt).unwrap();
    assert_eq!(info.cue_count, 2);
    assert_eq!(info.end_milliseconds, 2000);
    let vtt = b"WEBVTT\n\nenglish-1\n00:00.000 --> 00:01.500\nHello\n";
    assert_eq!(
        validate_subtitles(SubtitleFormat::WebVtt, vtt)
            .unwrap()
            .cue_count,
        1
    );
    for body in [
        "WEBVTT\n\n00:01.000 --> 00:00.000\nBad\n",
        "WEBVTT\n\n00:00.000 --> 00:01.000 position:50%\nUnsupported settings\n",
        "WEBVTT\n\n00:00.000 --> 00:01.000\n",
        "WEBVTT\n\n00:60.000 --> 01:01.000\nBad\n",
        "WEBVTT\n\nNOTE ignored\n",
        "WEBVTT\n\nSTYLE\n::cue { color: red }\n",
    ] {
        assert!(validate_subtitles(SubtitleFormat::WebVtt, body.as_bytes()).is_err());
    }
    assert!(
        validate_subtitles(
            SubtitleFormat::Srt,
            b"2\n00:00:00,000 --> 00:00:01,000\nMissing first cue\n"
        )
        .is_err()
    );
    assert_eq!(
        validate_subtitles(SubtitleFormat::Srt, &vec![b'a'; MAX_MANIFEST_BYTES + 1]),
        Err(Error::Limit)
    );
}

fn plan(path: PathBuf) -> MergePlan {
    MergePlan {
        tracks: vec![LocalTrack {
            selection: Default::default(),
            offset_milliseconds: 0,
            files: vec![path],
            container: SegmentContainer::MpegTs,
        }],
        subtitles: vec![],
        output: OutputContainer::Mp4,
        max_input_bytes: 1024,
    }
}

#[tokio::test]
async fn merge_rejects_relative_paths_and_observes_precancellation() {
    assert_eq!(
        plan(PathBuf::from("https://example.test/a")).validate(),
        Err(MergeError::Invalid)
    );
    let dir = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let error = remux_local(
        &dir.path().join("ffmpeg.exe"),
        &plan(dir.path().join("a.ts")),
        &cancel,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error, MergeError::Cancelled);
    #[cfg(windows)]
    assert_eq!(
        plan(PathBuf::from(r"\\server\share\a.ts")).validate(),
        Err(MergeError::Invalid)
    );
}

#[tokio::test]
async fn merge_enforces_staging_budget_before_invoking_program() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("a.ts");
    std::fs::write(&source, vec![1; 1025]).unwrap();
    let ffmpeg = std::env::current_exe().unwrap();
    let error = remux_local(&ffmpeg, &plan(source), &CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert_eq!(error, MergeError::Limit);
}

#[tokio::test]
async fn merge_rejects_invalid_subtitles_and_nonregular_inputs_before_spawn() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = std::env::current_exe().unwrap();
    let error = remux_local(
        &ffmpeg,
        &plan(dir.path().to_owned()),
        &CancellationToken::new(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error, MergeError::Invalid);
    let input = dir.path().join("track.ts");
    let subtitle = dir.path().join("subtitle.srt");
    std::fs::write(&input, b"bounded local bytes").unwrap();
    std::fs::write(&subtitle, b"not a subtitle").unwrap();
    let mut job = plan(input);
    job.subtitles.push(LocalSubtitle {
        path: subtitle,
        format: SubtitleFormat::Srt,
        language: Some("en".into()),
    });
    let error = remux_local(&ffmpeg, &job, &CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert_eq!(error, MergeError::Invalid);
}
