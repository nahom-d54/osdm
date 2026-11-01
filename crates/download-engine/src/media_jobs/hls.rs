//! Explicit selection: manifest references never grant network authority.
use super::*;
use crate::media_capture::{HlsRendition, HlsRenditionKind, TrackSelection, Variant};

pub fn hls_variant_key(variant: &Variant) -> String {
    format!(
        "hls-v-{:x}",
        Sha256::digest(serde_json::to_vec(variant).expect("HLS variant serialization"))
    )
}
pub fn hls_rendition_key(rendition: &HlsRendition) -> String {
    format!(
        "hls-r-{:x}",
        Sha256::digest(serde_json::to_vec(rendition).expect("HLS rendition serialization"))
    )
}
async fn playlist(
    engine: &Engine,
    url: &crate::media_capture::MediaUrl,
    request: &MediaJobRequest,
    origins: &HashSet<String>,
    secure: bool,
    token: &CancellationToken,
) -> Result<Vec<crate::media_capture::Segment>> {
    approve(origins, secure, url, "invalid HLS rendition playlist")?;
    let Manifest::Vod { segments } = engine
        .inspect_hls(url.as_str(), &request.allowed_origins, token)
        .await?
    else {
        return Err(invalid(
            "HLS renditions must be complete unencrypted VOD media playlists",
        ));
    };
    for segment in &segments {
        approve(
            origins,
            secure,
            &segment.source,
            "invalid HLS rendition segment",
        )?;
    }
    Ok(segments)
}
pub(super) async fn plan(
    engine: &Engine,
    request: &MediaJobRequest,
    origins: &HashSet<String>,
    secure: bool,
    token: &CancellationToken,
) -> Result<(Vec<SavedTrack>, Option<SavedSubtitle>)> {
    let Manifest::Master { variants } = engine
        .inspect_hls(&request.url, &request.allowed_origins, token)
        .await?
    else {
        return Err(invalid("HLS variant selection requires a master playlist"));
    };
    let variant = variants
        .iter()
        .find(|variant| Some(hls_variant_key(variant)).as_ref() == request.hls_variant_key.as_ref())
        .ok_or_else(|| invalid("HLS variant changed or is not present in this master"))?;
    let choose = |kind, key: &Option<String>| -> Result<Option<&HlsRendition>> {
        let Some(key) = key else {
            return Ok(None);
        };
        if key == "none" && kind == HlsRenditionKind::Audio {
            return Ok(None);
        }
        variant
            .renditions
            .iter()
            .find(|rendition| rendition.kind == kind && hls_rendition_key(rendition) == *key)
            .map(Some)
            .ok_or_else(|| invalid("HLS rendition is not part of the selected variant group"))
    };
    if variant.audio_group.is_some() && request.hls_audio_key.is_none() {
        return Err(invalid(
            "explicitly select HLS group audio or choose no external audio",
        ));
    }
    if variant.audio_group.is_none() && request.hls_audio_key.is_some() {
        return Err(invalid("selected variant has no HLS audio group"));
    }
    let audio = choose(HlsRenditionKind::Audio, &request.hls_audio_key)?;
    let subtitle = choose(HlsRenditionKind::Subtitle, &request.hls_subtitle_key)?;
    for url in std::iter::once(&variant.source)
        .chain(audio.and_then(|rendition| rendition.source.as_ref()))
        .chain(subtitle.and_then(|rendition| rendition.source.as_ref()))
    {
        approve(origins, secure, url, "invalid HLS selected playlist")?;
    }
    let video = playlist(engine, &variant.source, request, origins, secure, token).await?;
    let duration: f64 = video.iter().map(|segment| segment.duration_seconds).sum();
    let external_audio = audio.and_then(|rendition| rendition.source.as_ref());
    let mut tracks = vec![SavedTrack {
        sources: video
            .into_iter()
            .map(|segment| segment.source.as_str().to_owned())
            .collect(),
        container: TrackContainer::MpegTs,
        selection: if variant.audio_group.is_some() && (external_audio.is_some() || audio.is_none())
        {
            TrackSelection::Video
        } else {
            TrackSelection::All
        },
    }];
    if let Some(url) = external_audio {
        let segments = playlist(engine, url, request, origins, secure, token).await?;
        let audio_duration: f64 = segments
            .iter()
            .map(|segment| segment.duration_seconds)
            .sum();
        if (audio_duration - duration).abs() > 1.0 {
            return Err(invalid("HLS audio and video VOD durations do not match"));
        }
        tracks.push(SavedTrack {
            sources: segments
                .into_iter()
                .map(|segment| segment.source.as_str().to_owned())
                .collect(),
            container: TrackContainer::MpegTs,
            selection: TrackSelection::Audio,
        });
    }
    let subtitle = if let Some(rendition) = subtitle {
        let segments = playlist(
            engine,
            rendition
                .source
                .as_ref()
                .ok_or_else(|| invalid("external HLS subtitle URL is missing"))?,
            request,
            origins,
            secure,
            token,
        )
        .await?;
        let mut remaining = crate::media_capture::MAX_MANIFEST_BYTES;
        let mut bodies = Vec::new();
        for segment in segments {
            if remaining == 0 {
                return Err(crate::media_capture::Error::Limit.into());
            }
            let response = engine
                .fetch_site_page(segment.source.as_str(), remaining, token)
                .await?;
            if response.status != 200 {
                return Err(invalid(
                    "HLS subtitle segments require exact responses without redirects",
                ));
            }
            crate::media_capture::validate_hls_webvtt_segment(&response.body)?;
            remaining = remaining
                .checked_sub(response.body.len())
                .ok_or(crate::media_capture::Error::Limit)?;
            bodies.push(response.body);
        }
        Some(SavedSubtitle {
            format: crate::media_capture::SubtitleFormat::WebVtt,
            body: Vec::new(),
            hls_segments: bodies,
        })
    } else {
        None
    };
    Ok((tracks, subtitle))
}
pub(super) async fn start_pts(
    path: &Path,
    audio: bool,
    token: &CancellationToken,
    shutdown: &CancellationToken,
) -> Result<u64> {
    let file = tokio::fs::File::open(path).await?;
    let mut file = file.take(crate::media_capture::MAX_MANIFEST_BYTES as u64);
    let mut bytes = Vec::new();
    tokio::select! {
        biased;
        _ = token.cancelled() => return Err(Error::Interrupted),
        _ = shutdown.cancelled() => return Err(Error::Shutdown),
        result = file.read_to_end(&mut bytes) => { result?; }
    }
    Ok(crate::media_capture::mpegts_start_pts(&bytes, audio)?)
}
