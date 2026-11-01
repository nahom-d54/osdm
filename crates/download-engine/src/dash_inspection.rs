//! Static DASH inspection and explicit representation selection. Never selects quality.
use crate::{
    Engine, Error, Result,
    media_capture::{self, DashPlan, DashTrack, MediaUrl},
};
use std::{collections::HashSet, time::Duration};
use tokio_util::sync::CancellationToken;

/// Stable representation identity; never use a mutable manifest array index.
pub fn dash_track_key(track: &DashTrack) -> String {
    serde_json::to_string(&(&track.adaptation_id, &track.id)).expect("serializing string identity")
}

/// Resolve explicit A/V choices, rejecting duplicates and unsupported track semantics.
/// URL-bearing returned plans remain private inputs to engine-owned transfer jobs.
pub fn select_dash_tracks(plan: &DashPlan, keys: &[String]) -> Result<Vec<DashTrack>> {
    if keys.is_empty() || keys.len() > 2 {
        return Err(Error::Invalid(
            "choose one video and/or one audio representation".into(),
        ));
    }
    let mut chosen = HashSet::new();
    let mut kinds = HashSet::new();
    let mut tracks = Vec::new();
    let mut count = 0usize;
    for key in keys {
        if key.len() > 32768 || !chosen.insert(key) {
            return Err(Error::Invalid(
                "duplicate DASH representation choice".into(),
            ));
        }
        let matching: Vec<_> = plan
            .tracks
            .iter()
            .filter(|track| dash_track_key(track) == *key)
            .collect();
        if matching.len() != 1 {
            return Err(Error::Invalid(
                "selected DASH representation changed or is ambiguous".into(),
            ));
        }
        let track = matching[0];
        let kind = match track.mime_type.as_str() {
            "video/mp4" => "video",
            "audio/mp4" => "audio",
            _ => {
                return Err(Error::Invalid(
                    "only static fragmented MP4 DASH audio/video tracks are supported".into(),
                ));
            }
        };
        if !kinds.insert(kind) || track.initialization.is_none() || track.segments.is_empty() {
            return Err(Error::Invalid(
                "choose at most one initialized DASH track of each kind".into(),
            ));
        }
        count = count
            .checked_add(track.segments.len() + 1)
            .ok_or(media_capture::Error::Limit)?;
        if count > media_capture::MAX_ENTRIES {
            return Err(media_capture::Error::Limit.into());
        }
        tracks.push(track.clone());
    }
    Ok(tracks)
}

impl Engine {
    /// Inspect one conservative static DASH MPD using the same budgeted one-hop
    /// transport as Site Grabber. Redirects require explicit origin authorization;
    /// track/initialization references are returned, never fetched or authorized.
    pub async fn inspect_dash(
        &self,
        source: &str,
        allowed_origins: &[String],
        cancellation: &CancellationToken,
    ) -> Result<DashPlan> {
        let source = MediaUrl::parse(source)?;
        if allowed_origins.len() > 32 {
            return Err(media_capture::Error::Limit.into());
        }
        let source_origin = reqwest::Url::parse(source.as_str())
            .map_err(|_| media_capture::Error::UnsafeUrl)?
            .origin()
            .ascii_serialization();
        let mut approved = HashSet::from([source_origin]);
        for raw in allowed_origins {
            let url = MediaUrl::parse(raw)?;
            let origin = reqwest::Url::parse(url.as_str())
                .map_err(|_| media_capture::Error::UnsafeUrl)?
                .origin()
                .ascii_serialization();
            if origin != *raw {
                return Err(Error::Invalid(
                    "expected canonical approved HTTP(S) origins".into(),
                ));
            }
            approved.insert(origin);
        }
        let operation = async {
            let _active = self.inner.media.operations.read().await;
            if self.inner.data.lock().await.stopped {
                return Err(Error::Shutdown);
            }
            let mut current = source;
            let mut seen = HashSet::new();
            for _ in 0..=10 {
                if !seen.insert(current.clone()) {
                    return Err(Error::Invalid("DASH redirect loop".into()));
                }
                let response = self
                    .fetch_site_page(
                        current.as_str(),
                        media_capture::MAX_MANIFEST_BYTES,
                        cancellation,
                    )
                    .await?;
                if response.status == 200 {
                    return Ok(media_capture::inspect_dash(
                        current.as_str(),
                        &response.body,
                    )?);
                }
                if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
                    return Err(Error::Http(response.status));
                }
                let location = response
                    .location
                    .ok_or_else(|| Error::Invalid("invalid DASH redirect".into()))?;
                if location.len() > 8192
                    || location.trim() != location
                    || location.chars().any(char::is_control)
                {
                    return Err(media_capture::Error::UnsafeUrl.into());
                }
                let base = reqwest::Url::parse(current.as_str())
                    .map_err(|_| media_capture::Error::UnsafeUrl)?;
                let next = base
                    .join(&location)
                    .map_err(|_| media_capture::Error::UnsafeUrl)?;
                let checked = MediaUrl::parse(next.as_str())?;
                if (base.scheme() == "https" && next.scheme() != "https")
                    || !approved.contains(&next.origin().ascii_serialization())
                {
                    return Err(Error::Invalid(
                        "DASH redirect origin requires explicit approval".into(),
                    ));
                }
                current = checked;
            }
            Err(Error::Invalid("too many DASH redirects".into()))
        };
        tokio::select! {
            biased;
            _ = self.inner.media.shutdown.cancelled() => Err(Error::Shutdown),
            _ = cancellation.cancelled() => Err(Error::Interrupted),
            result = tokio::time::timeout(Duration::from_secs(60), operation) => result.unwrap_or_else(|_| Err(Error::Invalid("DASH inspection timed out".into()))),
        }
    }
}
