use osdm_media_capture::*;
const BASE: &str = "https://media.example.test/master.m3u8";
const MASTER: &str = "#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES,URI=\"audio.m3u8\"\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",NAME=\"Captions\",LANGUAGE=\"en\",URI=\"sub.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1000,AUDIO=\"a\",SUBTITLES=\"s\"\nvideo.m3u8\n";

#[test]
fn master_preserves_only_associated_renditions_without_selecting_defaults() {
    let Manifest::Master { variants } = inspect_hls(BASE, MASTER.as_bytes()).unwrap() else {
        panic!()
    };
    assert_eq!(variants[0].audio_group.as_deref(), Some("a"));
    assert_eq!(variants[0].renditions.len(), 2);
    assert!(variants[0].renditions[0].default);
    assert_eq!(
        variants[0].renditions[0].source.as_ref().unwrap().as_str(),
        "https://media.example.test/audio.m3u8"
    );
    assert!(!format!("{:?}", variants[0]).contains("English"));
    let in_band = MASTER.replace(",URI=\"audio.m3u8\"", "");
    let Manifest::Master { variants } = inspect_hls(BASE, in_band.as_bytes()).unwrap() else {
        panic!()
    };
    assert!(variants[0].renditions[0].source.is_none());
}

#[test]
fn invalid_groups_duplicates_unsafe_urls_and_unsupported_semantics_fail() {
    for body in [
        MASTER.replace("AUDIO=\"a\"", "AUDIO=\"absent\""),
        MASTER.replace(
            "LANGUAGE=\"en\",DEFAULT",
            "LANGUAGE=\"en\",NAME=\"duplicate\",DEFAULT",
        ),
        MASTER.replace("audio.m3u8", "http://media.example.test/audio.m3u8"),
        MASTER.replace(
            "audio.m3u8",
            "https://user:password@media.example.test/audio.m3u8",
        ),
        MASTER.replace("AUTOSELECT=YES", "AUTOSELECT=NO"),
        MASTER.replace("TYPE=SUBTITLES", "TYPE=CLOSED-CAPTIONS"),
        MASTER.replace(",URI=\"sub.m3u8\"", ""),
    ] {
        assert!(inspect_hls(BASE, body.as_bytes()).is_err());
    }
    for tag in [
        "#EXT-X-KEY:METHOD=AES-128,URI=\"key\"",
        "#EXT-X-MAP:URI=\"init\"",
        "#EXT-X-DISCONTINUITY",
    ] {
        assert!(
            inspect_hls(
                BASE,
                format!("#EXTM3U\n#EXT-X-TARGETDURATION:1\n{tag}\n#EXTINF:1,\na\n#EXT-X-ENDLIST\n")
                    .as_bytes()
            )
            .is_err()
        );
    }
}

#[test]
fn segmented_webvtt_uses_transport_maps_and_deduplicates_boundary_cues() {
    let first = b"WEBVTT\nX-TIMESTAMP-MAP=LOCAL:00:00.000,MPEGTS:900000\n\n00:00.500 --> 00:01.500\nFirst\n".to_vec();
    let second = b"WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:990000,LOCAL:00:01.000\n\n00:00.500 --> 00:01.500\nFirst\n\n00:01.500 --> 00:02.500\nSecond\n".to_vec();
    let bytes = merge_hls_webvtt(&[first, second], 900000).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert_eq!(text.matches("First").count(), 1);
    assert!(text.contains("00:00:00.500 --> 00:00:01.500"));
    assert!(text.contains("00:00:01.500 --> 00:00:02.500"));
    assert_eq!(
        validate_subtitles(SubtitleFormat::WebVtt, &bytes)
            .unwrap()
            .cue_count,
        2
    );
    assert_eq!(mpegts_clock_delta(90, (1 << 33) - 90).unwrap(), 180);
    assert!(mpegts_clock_delta(1 << 33, 0).is_err());
}

#[test]
fn malformed_maps_and_unbounded_subtitles_are_never_silently_flattened() {
    for bytes in [
        "WEBVTT\nX-TIMESTAMP-MAP=LOCAL:00:00.000,MPEGTS:8589934592\n\n",
        "WEBVTT\nX-TIMESTAMP-MAP=LOCAL:00:00.000,LOCAL:00:00.000,MPEGTS:0\n\n",
        "WEBVTT\nX-TIMESTAMP-MAP=LOCAL:00:00.000\n\n",
        "WEBVTT\n\nSTYLE\n::cue {color:red}\n",
        "WEBVTT\n\n00:00.000 --> 00:01.000 align:start\nCue\n",
    ] {
        assert!(validate_hls_webvtt_segment(bytes.as_bytes()).is_err());
    }
    assert!(merge_hls_webvtt(&[vec![b'x'; MAX_MANIFEST_BYTES + 1]], 0).is_err());
}

#[test]
fn transport_pts_reader_distinguishes_audio_and_video() {
    let mut packet = vec![0xff; 188];
    packet[..4].copy_from_slice(&[0x47, 0x40, 0x10, 0x10]);
    packet[4..18].copy_from_slice(&[0, 0, 1, 0xe0, 0, 0, 0x80, 0x80, 5, 0x21, 0, 5, 0xbf, 0x21]);
    assert_eq!(mpegts_start_pts(&packet, false).unwrap(), 90000);
    assert!(mpegts_start_pts(&packet, true).is_err());
    packet[7] = 0xc0;
    assert_eq!(mpegts_start_pts(&packet, true).unwrap(), 90000);
    packet[3] |= 0xc0;
    assert!(mpegts_start_pts(&packet, true).is_err());
}
