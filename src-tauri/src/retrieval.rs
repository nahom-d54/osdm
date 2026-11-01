//! Desktop DTOs and thin delegation only. Protocol/network state belongs to the engine.
use crate::{Reply, Shell};
use download_engine::{
    DownloadId,
    media_capture::{Manifest, MediaUrl},
};
use serde::Serialize;
use tauri::{Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize)]
pub(crate) struct MediaOffer {
    pub id: String,
    pub url: String,
    pub title: String,
    pub subtitle_url: Option<String>,
    pub kind: String,
}

#[derive(Default, Serialize)]
pub(crate) struct MediaChoice {
    url: String,
    bandwidth: String,
    resolution: Option<(u32, u32)>,
    codecs: Option<String>,
    track_key: Option<String>,
    track_kind: Option<&'static str>,
    language: Option<String>,
    segment_count: usize,
}

#[derive(Serialize)]
pub(crate) struct MediaInspection {
    kind: &'static str,
    choices: Vec<MediaChoice>,
    segment_count: usize,
    resource_origins: Vec<String>,
}

#[tauri::command]
pub(crate) async fn engine_inspect_media(
    shell: State<'_, Shell>,
    url: String,
    allowed_origins: Vec<String>,
    request_id: String,
    format: Option<String>,
) -> Reply<MediaInspection> {
    DownloadId::parse_str(&request_id).map_err(|_| "Invalid inspection ID")?;
    let dash = match format.as_deref() {
        None | Some("hls") => false,
        Some("dash") => true,
        _ => return Err("Unsupported media format".into()),
    };
    let engine = shell.engine().await?;
    let token = CancellationToken::new();
    {
        let mut active = shell.media_inspections.lock().await;
        if active
            .get(&request_id)
            .is_some_and(CancellationToken::is_cancelled)
        {
            active.remove(&request_id);
            return Err("operation interrupted".into());
        }
        if active
            .values()
            .filter(|token| !token.is_cancelled())
            .count()
            >= 8
            || active.contains_key(&request_id)
        {
            return Err("Too many active media inspections".into());
        }
        active.insert(request_id.clone(), token.clone());
    }
    if dash {
        let result = engine.inspect_dash(&url, &allowed_origins, &token).await;
        shell.media_inspections.lock().await.remove(&request_id);
        return dash_view(result.map_err(|error| error.to_string())?);
    }
    let result = engine.inspect_hls(&url, &allowed_origins, &token).await;
    shell.media_inspections.lock().await.remove(&request_id);
    match result.map_err(|error| error.to_string())? {
        Manifest::Master { variants } => Ok(MediaInspection {
            kind: "master",
            choices: variants
                .into_iter()
                .map(|variant| MediaChoice {
                    url: variant.source.as_str().into(),
                    bandwidth: variant.bandwidth.to_string(),
                    resolution: variant.resolution,
                    codecs: variant.codecs,
                    ..MediaChoice::default()
                })
                .collect(),
            segment_count: 0,
            resource_origins: Vec::new(),
        }),
        Manifest::Vod { segments } => {
            let mut origins = std::collections::BTreeSet::new();
            for segment in &segments {
                if let Ok(url) = reqwest::Url::parse(segment.source.as_str()) {
                    origins.insert(url.origin().ascii_serialization());
                }
            }
            Ok(MediaInspection {
                kind: "vod",
                choices: Vec::new(),
                segment_count: segments.len(),
                resource_origins: origins.into_iter().collect(),
            })
        }
    }
}

fn dash_view(plan: download_engine::media_capture::DashPlan) -> Reply<MediaInspection> {
    let mut origins = std::collections::BTreeSet::new();
    let mut choices = Vec::new();
    let mut count = 0;
    for track in plan.tracks {
        for source in track
            .initialization
            .iter()
            .chain(track.segments.iter().map(|segment| &segment.source))
        {
            let url = reqwest::Url::parse(source.as_str()).map_err(|_| "Invalid DASH resource")?;
            origins.insert(url.origin().ascii_serialization());
        }
        let key = download_engine::dash_track_key(&track);
        let resolution = match (track.width, track.height) {
            (Some(width), Some(height)) => Some((
                u32::try_from(width).map_err(|_| "Unsupported DASH dimensions")?,
                u32::try_from(height).map_err(|_| "Unsupported DASH dimensions")?,
            )),
            _ => None,
        };
        let kind = match track.mime_type.as_str() {
            "video/mp4" => "video",
            "audio/mp4" => "audio",
            "text/vtt" | "application/x-subrip" => "subtitle",
            _ => return Err("Unsupported DASH track".into()),
        };
        let url =
            if kind == "subtitle" && track.segments.len() == 1 && track.initialization.is_none() {
                track.segments[0].source.as_str().into()
            } else {
                plan.manifest.as_str().into()
            };
        count += track.segments.len();
        choices.push(MediaChoice {
            url,
            bandwidth: track.bandwidth.to_string(),
            resolution,
            codecs: track.codecs,
            track_key: Some(key),
            track_kind: Some(kind),
            language: track.language,
            segment_count: track.segments.len(),
        });
    }
    Ok(MediaInspection {
        kind: "dash",
        choices,
        segment_count: count,
        resource_origins: origins.into_iter().collect(),
    })
}

#[tauri::command]
pub(crate) async fn engine_cancel_inspection(
    shell: State<'_, Shell>,
    request_id: String,
) -> Reply<()> {
    if DownloadId::parse_str(&request_id).is_err() {
        return Ok(());
    }
    let mut active = shell.media_inspections.lock().await;
    if let Some(token) = active.get(&request_id) {
        token.cancel();
    } else if active.len() < 64 {
        // Cancellation can beat command admission while engine() waits. Retain a
        // bounded tombstone so a closed dialog never starts a late request.
        let token = CancellationToken::new();
        token.cancel();
        active.insert(request_id, token);
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn media_offers(shell: State<'_, Shell>) -> Reply<Vec<MediaOffer>> {
    Ok(shell.media_offers.lock().await.clone())
}

#[tauri::command]
pub(crate) async fn media_dismiss_offer(shell: State<'_, Shell>, id: String) -> Reply<()> {
    shell
        .media_offers
        .lock()
        .await
        .retain(|offer| offer.id != id);
    Ok(())
}

#[tauri::command]
pub(crate) async fn engine_create_media_job(
    shell: State<'_, Shell>,
    request: download_engine::MediaJobRequest,
) -> Reply<DownloadId> {
    shell
        .engine()
        .await?
        .create_media_job(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn engine_media_jobs(
    shell: State<'_, Shell>,
) -> Reply<Vec<download_engine::MediaJob>> {
    Ok(shell.engine().await?.list_media_jobs().await)
}

#[tauri::command]
pub(crate) async fn engine_control_media_job(
    shell: State<'_, Shell>,
    id: DownloadId,
    action: String,
) -> Reply<()> {
    let engine = shell.engine().await?;
    match action.as_str() {
        "pause" => engine.pause_media_job(id).await,
        "resume" => engine.resume_media_job(id).await,
        "cancel" => engine.cancel_media_job(id).await,
        _ => return Err("Unknown media action".into()),
    }
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn engine_create_site_project(
    shell: State<'_, Shell>,
    request: download_engine::SiteProjectRequest,
) -> Reply<DownloadId> {
    shell
        .engine()
        .await?
        .create_site_project(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn engine_site_projects(
    shell: State<'_, Shell>,
) -> Reply<Vec<download_engine::SiteProject>> {
    Ok(shell.engine().await?.list_site_projects().await)
}

#[tauri::command]
pub(crate) async fn engine_control_site_project(
    shell: State<'_, Shell>,
    id: DownloadId,
    action: download_engine::SiteAction,
) -> Reply<()> {
    shell
        .engine()
        .await?
        .control_site_project(id, action)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn engine_queue_site_resources(
    shell: State<'_, Shell>,
    id: DownloadId,
    resources: Vec<String>,
) -> Reply<Vec<DownloadId>> {
    shell
        .engine()
        .await?
        .queue_site_resources(id, resources)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn offer(
    app: &tauri::AppHandle,
    url: String,
    title: String,
    subtitle_url: Option<String>,
    kind: Option<String>,
) -> Result<(), &'static str> {
    let mut offer = validated_offer(url, title, subtitle_url)?;
    if let Some(kind) = kind {
        if !matches!(kind.as_str(), "hls" | "dash" | "direct") {
            return Err("invalid_media_kind");
        }
        offer.kind = kind;
    }
    let shell = app.state::<Shell>();
    let mut offers = shell.media_offers.lock().await;
    if let Some(old) = offers.iter_mut().find(|old| old.url == offer.url) {
        old.title = offer.title;
        old.subtitle_url = offer.subtitle_url;
        old.kind = offer.kind;
    } else {
        if offers.len() >= 32 {
            return Err("too_many_media_offers");
        }
        offers.push(offer);
    }
    drop(offers);
    app.emit("media://offers-changed", ())
        .map_err(|_| "media_offer_failed")?;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
    }
    Ok(())
}

fn validated_offer(
    url: String,
    title: String,
    subtitle_url: Option<String>,
) -> Result<MediaOffer, &'static str> {
    let url = MediaUrl::parse(&url).map_err(|_| "invalid_media_url")?;
    if title.len() > 1024 || title.chars().any(char::is_control) {
        return Err("invalid_media_title");
    }
    if let Some(subtitle) = &subtitle_url {
        MediaUrl::parse(subtitle).map_err(|_| "invalid_subtitle_url")?;
    }
    let path = reqwest::Url::parse(url.as_str())
        .map_err(|_| "invalid_media_url")?
        .path()
        .to_ascii_lowercase();
    let kind = if path.ends_with(".m3u8") {
        "hls"
    } else if path.ends_with(".mpd") {
        "dash"
    } else {
        "unknown"
    };
    Ok(MediaOffer {
        id: DownloadId::new_v4().to_string(),
        url: url.as_str().into(),
        title,
        subtitle_url,
        kind: kind.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_offers_validate_all_sources_and_bound_untrusted_titles() {
        assert!(
            validated_offer(
                "https://media.example.test/movie.m3u8".into(),
                "A film".into(),
                Some("https://media.example.test/captions.vtt".into())
            )
            .is_ok()
        );
        for source in [
            "file:///movie",
            "https://user:password@example.test/movie",
            "https://example.test/movie#fragment",
            "https://example.test/\nsynthetic-secret",
        ] {
            assert_eq!(
                validated_offer(source.into(), "A film".into(), None).err(),
                Some("invalid_media_url")
            );
            assert_eq!(
                validated_offer(
                    "https://media.example.test/movie.m3u8".into(),
                    "A film".into(),
                    Some(source.into())
                )
                .err(),
                Some("invalid_subtitle_url")
            );
        }
        for title in ["x".repeat(1025), "film\nsynthetic-secret".into()] {
            assert_eq!(
                validated_offer("https://media.example.test/movie.m3u8".into(), title, None).err(),
                Some("invalid_media_title")
            );
        }
    }
}
