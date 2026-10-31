use osdm_media_capture::{Error, MAX_ENTRIES, MAX_MANIFEST_BYTES, Manifest, inspect_hls};

const BASE: &str = "https://media.example.test/watch/index.m3u8";

#[test]
fn master_preserves_quality_order_and_quoted_codec_commas() {
    let manifest = inspect_hls(BASE, b"#EXTM3U\r\n#EXT-X-VERSION:3\r\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"avc1.4d401e,mp4a.40.2\"\r\nlow/list.m3u8\r\n#EXT-X-STREAM-INF:BANDWIDTH=2400000,RESOLUTION=1280x720\r\n/high.m3u8\r\n").unwrap();
    let Manifest::Master { variants } = manifest else {
        panic!("expected choices")
    };
    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0].bandwidth, 800_000);
    assert_eq!(variants[0].resolution, Some((640, 360)));
    assert_eq!(variants[0].codecs.as_deref(), Some("avc1.4d401e,mp4a.40.2"));
    assert_eq!(
        variants[0].source.as_str(),
        "https://media.example.test/watch/low/list.m3u8"
    );
    assert_eq!(
        variants[1].source.as_str(),
        "https://media.example.test/high.m3u8"
    );
}

#[test]
fn complete_vod_preserves_segment_order_and_durations() {
    let manifest = inspect_hls(BASE, b"#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:42\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:5.5,First\na.ts\n#EXTINF:2.25,\n../b.ts\n#EXT-X-ENDLIST\n").unwrap();
    let Manifest::Vod { segments } = manifest else {
        panic!("expected segments")
    };
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].duration_seconds, 5.5);
    assert_eq!(segments[1].duration_seconds, 2.25);
    assert_eq!(
        segments[1].source.as_str(),
        "https://media.example.test/b.ts"
    );
}

#[test]
fn unsupported_semantics_are_not_silently_ignored() {
    for tag in [
        "#EXT-X-KEY:METHOD=AES-128,URI=\"key\"",
        "#EXT-X-BYTERANGE:100@0",
        "#EXT-X-MAP:URI=\"init.mp4\"",
        "#EXT-X-DISCONTINUITY",
        "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\"",
        "#EXT-X-VERSION:7",
        "#EXT-X-UNKNOWN:1",
    ] {
        let body = format!("#EXTM3U\n{tag}\n");
        assert_eq!(inspect_hls(BASE, body.as_bytes()), Err(Error::Unsupported));
    }
    assert_eq!(
        inspect_hls(
            BASE,
            b"#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:5,\na.ts\n"
        ),
        Err(Error::Unsupported)
    );
    assert_eq!(
        inspect_hls(
            BASE,
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"a\"\na.m3u8\n"
        ),
        Err(Error::Unsupported)
    );
}

#[test]
fn malformed_and_mixed_playlists_fail() {
    for body in [
        "not a manifest",
        "#EXTM3U\n",
        "#EXTM3U\na.ts\n",
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n",
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n#EXT-X-STREAM-INF:BANDWIDTH=2\na\n",
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\na\n#EXT-X-TARGETDURATION:6\n",
        "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-STREAM-INF:BANDWIDTH=1\na\n",
        "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:1,\n",
        "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:1,\na\n#EXT-X-ENDLIST\nb\n",
        "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:1,\na\n#EXT-X-ENDLIST\n#EXT-X-ENDLIST\n",
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-VERSION:3\n",
    ] {
        assert_eq!(inspect_hls(BASE, body.as_bytes()), Err(Error::Invalid));
    }
    assert_eq!(inspect_hls(BASE, &[0xff]), Err(Error::Invalid));
}

#[test]
fn invalid_numbers_and_attributes_fail() {
    for attrs in [
        "BANDWIDTH=0",
        "BANDWIDTH=-1",
        "BANDWIDTH=+1",
        "BANDWIDTH=18446744073709551616",
        "BANDWIDTH=1,BANDWIDTH=2",
        "BANDWIDTH=\"1\"",
        "RESOLUTION=1x1",
        "BANDWIDTH=1,RESOLUTION=0x720",
        "BANDWIDTH=1,RESOLUTION=4294967296x720",
        "BANDWIDTH=1,CODECS=\"unclosed",
        "BANDWIDTH=1,",
        "BANDWIDTH=1,CODECS=unquoted",
    ] {
        let body = format!("#EXTM3U\n#EXT-X-STREAM-INF:{attrs}\na.m3u8\n");
        assert_eq!(inspect_hls(BASE, body.as_bytes()), Err(Error::Invalid));
    }
    for duration in ["NaN", "inf", "-1", "0", "7", "1e999", "1e0", "+1"] {
        let body =
            format!("#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:{duration},\na\n#EXT-X-ENDLIST\n");
        assert_eq!(inspect_hls(BASE, body.as_bytes()), Err(Error::Invalid));
    }
}

#[test]
fn unsafe_sources_and_downgrades_fail() {
    for source in [
        "file:///tmp/a",
        "javascript:alert(1)",
        "data:text/plain,a",
        "https://user:password@example.test/a",
        "http://example.test/a",
        "a#fragment",
    ] {
        let body = format!("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n{source}\n");
        assert_eq!(inspect_hls(BASE, body.as_bytes()), Err(Error::UnsafeUrl));
    }
    for base in [
        "file:///a",
        "https://user:password@example.test/a",
        "https://example.test/a#fragment",
    ] {
        assert_eq!(inspect_hls(base, b"#EXTM3U\n"), Err(Error::UnsafeUrl));
    }
}

#[test]
fn query_is_not_inherited_and_debug_is_redacted() {
    let Manifest::Master { variants } = inspect_hls(
        "https://media.example.test/index.m3u8?",
        b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nchild.m3u8\n",
    )
    .unwrap() else {
        panic!("expected choices")
    };
    assert_eq!(
        variants[0].source.as_str(),
        "https://media.example.test/child.m3u8"
    );
    let debug = format!("{variants:?}");
    assert!(!debug.contains("example.test"));
    assert!(!debug.contains("child.m3u8"));
    assert!(debug.contains("[redacted]"));
}

#[test]
fn bounds_are_enforced() {
    assert_eq!(
        inspect_hls(BASE, &vec![b'a'; MAX_MANIFEST_BYTES + 1]),
        Err(Error::Limit)
    );
    let long_line = format!("#EXTM3U\n#{}", "a".repeat(8192));
    assert_eq!(inspect_hls(BASE, long_line.as_bytes()), Err(Error::Limit));
    let mut body = String::from("#EXTM3U\n");
    for _ in 0..MAX_ENTRIES {
        body.push_str("#EXT-X-STREAM-INF:BANDWIDTH=1\na.m3u8\n");
    }
    assert!(inspect_hls(BASE, body.as_bytes()).is_ok());
    body.push_str("#EXT-X-STREAM-INF:BANDWIDTH=1\na.m3u8\n");
    assert_eq!(inspect_hls(BASE, body.as_bytes()), Err(Error::Limit));
    let vod = format!(
        "#EXTM3U\n#EXT-X-TARGETDURATION:1\n{}#EXT-X-ENDLIST\n",
        "#EXTINF:1,\na.ts\n".repeat(MAX_ENTRIES + 1)
    );
    assert_eq!(inspect_hls(BASE, vod.as_bytes()), Err(Error::Limit));
}
