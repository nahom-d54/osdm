//! Durable, explicitly selected, unauthenticated MPEG-TS HLS VOD jobs.
//! Ordinary transfer workers own every segment; ffmpeg sees local files only.
use crate::{
    DownloadId, DownloadRequest, DownloadState, Engine, Error, Result, engine::same_destination,
    media_capture::Manifest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

pub type MediaJobId = uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct MediaJobRequest {
    pub url: String,
    pub destination: PathBuf,
    pub allowed_origins: Vec<String>,
    pub ffmpeg_path: PathBuf,
    pub description: Option<String>,
    #[serde(default)]
    pub subtitle_url: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
    /// Explicit DASH representation choices from `inspect_dash`; empty means HLS.
    #[serde(default)]
    pub dash_tracks: Vec<String>,
    /// Explicit local-container remux of one direct MP4/MOV file (for subtitles).
    #[serde(default)]
    pub direct_media: bool,
}

impl std::fmt::Debug for MediaJobRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaJobRequest")
            .field("url", &"[redacted]")
            .field("destination", &self.destination)
            .field("dash_tracks", &self.dash_tracks.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaJobState {
    Paused,
    Downloading,
    Merging,
    Publishing,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaJob {
    pub id: MediaJobId,
    pub destination: PathBuf,
    pub description: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
    pub state: MediaJobState,
    pub children: Vec<DownloadId>,
    pub completed_segments: usize,
    pub total_segments: usize,
    /// A locally computed content hash, not publisher-authenticated integrity.
    pub sha256: Option<String>,
    pub error: Option<String>,
}

/// One merge input group. Fragmented MP4 keeps its initialization segment first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TrackContainer {
    MpegTs,
    FragmentedMp4,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SavedTrack {
    sources: Vec<String>,
    container: TrackContainer,
}

// Deliberately no Debug: source URLs (including query credentials) are secrets.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SavedJob {
    pub view: MediaJob,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tracks: Vec<SavedTrack>,
    /// Pre-DASH payloads stored one flat TS source list; migrated on load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sources: Vec<String>,
    ffmpeg: PathBuf,
    directory: PathBuf,
    marker: String,
    artifact: Option<PathBuf>,
    ready: bool,
    length: Option<u64>,
    #[serde(default)]
    subtitle: Option<SavedSubtitle>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedSubtitle {
    format: crate::media_capture::SubtitleFormat,
    body: Vec<u8>,
}

impl SavedJob {
    pub(crate) fn owns_child(&self, download: &crate::Download) -> bool {
        self.view.children.contains(&download.id)
            || download.destination.parent() == Some(self.directory.as_path())
    }

    /// Upgrade an older single-track payload without changing any child identity.
    pub(crate) fn migrate_legacy(&mut self) {
        if self.tracks.is_empty() && !self.sources.is_empty() {
            self.tracks.push(SavedTrack {
                sources: std::mem::take(&mut self.sources),
                container: TrackContainer::MpegTs,
            });
        }
    }

    fn total_sources(&self) -> usize {
        self.tracks.iter().map(|track| track.sources.len()).sum()
    }

    fn source_at(&self, flat: usize) -> Option<(&str, TrackContainer)> {
        let mut remaining = flat;
        for track in &self.tracks {
            if remaining < track.sources.len() {
                return Some((track.sources[remaining].as_str(), track.container));
            }
            remaining -= track.sources.len();
        }
        None
    }

    fn child_path(&self, flat: usize) -> Option<PathBuf> {
        let (_, container) = self.source_at(flat)?;
        let extension = match container {
            TrackContainer::MpegTs => "ts",
            TrackContainer::FragmentedMp4 => "m4s",
        };
        Some(self.directory.join(format!("{flat:08}.{extension}")))
    }
}

fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}

/// Manifest references are not network authorization. Every fetched media
/// resource must match an explicitly approved origin and never downgrade TLS.
fn approve(
    origins: &HashSet<String>,
    secure_source: bool,
    source: &crate::media_capture::MediaUrl,
    what: &str,
) -> Result<()> {
    let url = reqwest::Url::parse(source.as_str()).map_err(|_| invalid(what))?;
    if !origins.contains(&url.origin().ascii_serialization()) {
        return Err(invalid("media resource origin requires explicit approval"));
    }
    if secure_source && url.scheme() != "https" {
        return Err(invalid("media HTTPS downgrade is unsupported"));
    }
    Ok(())
}

impl Engine {
    async fn acquire_media_processor(
        &self,
        token: &CancellationToken,
    ) -> Result<tokio::sync::OwnedSemaphorePermit> {
        tokio::select! {
            biased;
            _ = token.cancelled() => Err(Error::Interrupted),
            _ = self.inner.media.shutdown.cancelled() => Err(Error::Shutdown),
            permit = self.inner.media_processing.clone().acquire_owned() => permit.map_err(|_| Error::Shutdown),
        }
    }
    /// The caller must first inspect a master and explicitly select a variant.
    /// Never guesses quality or authorizes origins merely because a manifest names them.
    pub async fn create_media_job(&self, request: MediaJobRequest) -> Result<MediaJobId> {
        let _operation = self.inner.media.operations.read().await;
        if self.inner.media.shutdown.is_cancelled() {
            return Err(Error::Shutdown);
        }
        if !request.destination.is_absolute()
            || request.destination.file_name().is_none()
            || !matches!(
                request.destination.extension().and_then(|s| s.to_str()),
                Some("mkv" | "mp4")
            )
        {
            return Err(invalid(
                "media destination must be an absolute .mkv or .mp4 filename",
            ));
        }
        if !request.ffmpeg_path.is_absolute() {
            return Err(invalid(
                "configure an absolute path to an installed ffmpeg executable",
            ));
        }
        if !tokio::fs::metadata(&request.ffmpeg_path)
            .await
            .map_err(|_| invalid("configured ffmpeg executable is unavailable"))?
            .is_file()
        {
            return Err(invalid("configured ffmpeg executable is unavailable"));
        }
        crate::DownloadProperties {
            description: request.description.clone().unwrap_or_default(),
            category_id: request.category_id.clone(),
        }
        .validate()?;
        let source = crate::media_capture::MediaUrl::parse(&request.url)?;
        let secure_source = source.as_str().starts_with("https:");
        let source_origin = reqwest::Url::parse(source.as_str())
            .map_err(|_| invalid("invalid media origin"))?
            .origin()
            .ascii_serialization();
        let mut origins = HashSet::from([source_origin]);
        for allowed in &request.allowed_origins {
            let url = reqwest::Url::parse(allowed)
                .map_err(|_| invalid("invalid approved media origin"))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.origin().ascii_serialization() != *allowed
            {
                return Err(invalid("expected canonical approved HTTP(S) origins"));
            }
            origins.insert(allowed.clone());
        }
        let cancellation = CancellationToken::new();
        let tracks = if request.direct_media {
            if !request.dash_tracks.is_empty() {
                return Err(invalid("direct media cannot select DASH representations"));
            }
            let url = reqwest::Url::parse(source.as_str())
                .map_err(|_| invalid("invalid direct media URL"))?;
            let extension = Path::new(url.path())
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(extension.as_str(), "mp4" | "m4v" | "mov") {
                return Err(invalid(
                    "direct subtitle remux supports MP4/M4V/MOV; other direct files can be saved without remux",
                ));
            }
            vec![SavedTrack {
                sources: vec![source.as_str().to_owned()],
                container: TrackContainer::FragmentedMp4,
            }]
        } else if request.dash_tracks.is_empty() {
            let segments = match self
                .inspect_hls(source.as_str(), &request.allowed_origins, &cancellation)
                .await?
            {
                Manifest::Vod { segments } => segments,
                Manifest::Master { .. } => {
                    return Err(invalid(
                        "select an explicit HLS quality before creating a media job",
                    ));
                }
            };
            for segment in &segments {
                approve(
                    &origins,
                    secure_source,
                    &segment.source,
                    "invalid media segment",
                )?;
            }
            vec![SavedTrack {
                sources: segments
                    .into_iter()
                    .map(|segment| segment.source.as_str().to_owned())
                    .collect(),
                container: TrackContainer::MpegTs,
            }]
        } else {
            // An explicit video/audio representation choice is mandatory; never
            // infer a quality from bandwidth or manifest order.
            let plan = self
                .inspect_dash(source.as_str(), &request.allowed_origins, &cancellation)
                .await?;
            let selected = crate::select_dash_tracks(&plan, &request.dash_tracks)?;
            let mut tracks = Vec::new();
            for track in selected {
                let mut sources = Vec::new();
                if let Some(initialization) = &track.initialization {
                    approve(
                        &origins,
                        secure_source,
                        initialization,
                        "invalid DASH initialization segment",
                    )?;
                    sources.push(initialization.as_str().to_owned());
                }
                for segment in &track.segments {
                    approve(
                        &origins,
                        secure_source,
                        &segment.source,
                        "invalid DASH segment",
                    )?;
                    sources.push(segment.source.as_str().to_owned());
                }
                if sources.is_empty()
                    || sources.len() > crate::media_capture::MAX_ENTRIES.saturating_div(2)
                {
                    return Err(invalid("unsupported DASH track size"));
                }
                tracks.push(SavedTrack {
                    sources,
                    container: TrackContainer::FragmentedMp4,
                });
            }
            tracks
        };
        let total_sources: usize = tracks.iter().map(|track| track.sources.len()).sum();
        if total_sources == 0 || total_sources > crate::media_capture::MAX_ENTRIES {
            return Err(crate::media_capture::Error::Limit.into());
        }
        let subtitle = if let Some(source) = &request.subtitle_url {
            let source = crate::media_capture::MediaUrl::parse(source)?;
            let url = reqwest::Url::parse(source.as_str())
                .map_err(|_| invalid("invalid subtitle source"))?;
            if !origins.contains(&url.origin().ascii_serialization()) {
                return Err(invalid("subtitle origin requires explicit approval"));
            }
            if secure_source && url.scheme() != "https" {
                return Err(invalid("subtitle HTTPS downgrade is unsupported"));
            }
            let extension = Path::new(url.path())
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let format = match extension.as_str() {
                "srt" => crate::media_capture::SubtitleFormat::Srt,
                "vtt" => crate::media_capture::SubtitleFormat::WebVtt,
                _ => {
                    return Err(invalid(
                        "only explicitly selected .srt or .vtt subtitles are supported",
                    ));
                }
            };
            let response = self
                .fetch_site_page(
                    source.as_str(),
                    crate::media_capture::MAX_MANIFEST_BYTES,
                    &cancellation,
                )
                .await?;
            if response.status != 200 {
                return Err(invalid(
                    "subtitle fetch failed; subtitle redirects are unsupported",
                ));
            }
            crate::media_capture::validate_subtitles(format, &response.body)?;
            Some(SavedSubtitle {
                format,
                body: response.body,
            })
        } else {
            None
        };
        let parent = tokio::fs::canonicalize(request.destination.parent().unwrap()).await?;
        let destination = parent.join(request.destination.file_name().unwrap());
        if tokio::fs::try_exists(&destination).await? {
            return Err(invalid("destination already exists"));
        }
        let id = MediaJobId::new_v4();
        let job = SavedJob {
            view: MediaJob {
                id,
                destination,
                description: request.description,
                category_id: request.category_id,
                state: MediaJobState::Paused,
                children: vec![],
                completed_segments: 0,
                total_segments: total_sources,
                sha256: None,
                error: None,
            },
            tracks,
            sources: Vec::new(),
            ffmpeg: tokio::fs::canonicalize(request.ffmpeg_path).await?,
            directory: parent.join(format!(".osdm-media-{id}")),
            marker: uuid::Uuid::new_v4().to_string(),
            artifact: None,
            ready: false,
            length: None,
            subtitle,
        };
        {
            let mut data = self.inner.data.lock().await;
            if data.stopped {
                return Err(Error::Shutdown);
            }
            if data
                .downloads
                .iter()
                .any(|d| same_destination(&d.destination, &job.view.destination))
                || data
                    .media_jobs
                    .iter()
                    .any(|j| same_destination(&j.view.destination, &job.view.destination))
            {
                return Err(invalid("destination is already reserved"));
            }
            // Persist ownership intent before creating directories, markers, or children.
            self.inner.store.save_media_job(&job).await?;
            data.media_jobs.push(job);
        }
        self.resume_media_job(id).await?;
        Ok(id)
    }

    pub async fn list_media_jobs(&self) -> Vec<MediaJob> {
        let data = self.inner.data.lock().await;
        data.media_jobs
            .iter()
            .map(|job| {
                let mut view = job.view.clone();
                view.completed_segments = view
                    .children
                    .iter()
                    .filter(|id| {
                        data.downloads
                            .iter()
                            .any(|d| d.id == **id && d.state == DownloadState::Completed)
                    })
                    .count();
                view
            })
            .collect()
    }

    pub async fn resume_media_job(&self, id: MediaJobId) -> Result<()> {
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        if data.media_running.contains_key(&id) {
            return Ok(());
        }
        let job = data
            .media_jobs
            .iter_mut()
            .find(|j| j.view.id == id)
            .ok_or(Error::NotFound)?;
        if job.view.state == MediaJobState::Completed {
            return Ok(());
        }
        job.view.state = MediaJobState::Downloading;
        job.view.error = None;
        self.inner.store.save_media_job(job).await?;
        let token = CancellationToken::new();
        data.media_running.insert(id, token.clone());
        let engine = self.clone();
        tokio::spawn(async move {
            engine.run_media_job(id, token).await;
        });
        Ok(())
    }

    pub async fn pause_media_job(&self, id: MediaJobId) -> Result<()> {
        self.stop_media_job(id, false).await
    }
    pub async fn cancel_media_job(&self, id: MediaJobId) -> Result<()> {
        self.stop_media_job(id, true).await
    }

    async fn stop_media_job(&self, id: MediaJobId, cancel: bool) -> Result<()> {
        {
            let mut data = self.inner.data.lock().await;
            if data.stopped {
                return Err(Error::Shutdown);
            }
            let job = data
                .media_jobs
                .iter_mut()
                .find(|j| j.view.id == id)
                .ok_or(Error::NotFound)?;
            if job.view.state == MediaJobState::Completed {
                return Ok(());
            }
            job.view.state = if cancel {
                MediaJobState::Canceled
            } else {
                MediaJobState::Paused
            };
            self.inner.store.save_media_job(job).await?;
            if let Some(token) = data.media_running.get(&id) {
                token.cancel();
            }
        }
        while self.inner.data.lock().await.media_running.contains_key(&id) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }

    async fn saved_media_job(&self, id: MediaJobId) -> Result<SavedJob> {
        self.inner
            .data
            .lock()
            .await
            .media_jobs
            .iter()
            .find(|j| j.view.id == id)
            .cloned()
            .ok_or(Error::NotFound)
    }

    async fn run_media_job(&self, id: MediaJobId, token: CancellationToken) {
        // Isolate panics without abandoning subprocesses/children on normal cancellation.
        let engine = self.clone();
        let worker_token = token.clone();
        let result =
            tokio::spawn(async move { engine.transfer_media_job(id, &worker_token).await })
                .await
                .unwrap_or_else(|_| Err(invalid("media worker terminated unexpectedly")));
        // Stop and drain all child workers, including when a later segment fails.
        if let Ok(job) = self.saved_media_job(id).await {
            for child in &job.view.children {
                if self.inner.data.lock().await.stopped {
                    self.wait_idle(*child).await;
                } else {
                    let _ = self
                        .stop(*child, job.view.state == MediaJobState::Canceled)
                        .await;
                }
            }
        }
        let mut data = self.inner.data.lock().await;
        if let Some(job) = data.media_jobs.iter_mut().find(|j| j.view.id == id) {
            if job.view.state != MediaJobState::Completed
                && !matches!(
                    job.view.state,
                    MediaJobState::Paused | MediaJobState::Canceled
                )
            {
                job.view.state = if token.is_cancelled() || self.inner.media.shutdown.is_cancelled()
                {
                    MediaJobState::Paused
                } else {
                    MediaJobState::Failed
                };
                if let Err(error) = result
                    && !matches!(error, Error::Interrupted | Error::Shutdown)
                {
                    job.view.error = Some(error.to_string());
                }
            }
            if let Err(error) = self.inner.store.save_media_job(job).await {
                let _ = self
                    .inner
                    .events
                    .send(crate::EngineEvent::Error(error.to_string()));
            }
        }
        data.media_running.remove(&id);
    }

    async fn transfer_media_job(&self, id: MediaJobId, token: &CancellationToken) -> Result<()> {
        let mut job = self.saved_media_job(id).await?;
        check_cancel(self, token)?;
        validate_layout(&job)?;
        owned_directory(&job).await?;
        // Recovery of publication is separate from remuxing: never replace an
        // existing name, and require both file identity and the persisted hash.
        if job.ready {
            return self.publish_media_job(id, token).await;
        }
        for index in 0..job.total_sources() {
            check_cancel(self, token)?;
            let (source, container) = job
                .source_at(index)
                .map(|(url, container)| (url.to_owned(), container))
                .ok_or_else(|| invalid("media plan is missing a source"))?;
            let destination = job
                .child_path(index)
                .ok_or_else(|| invalid("media plan is missing a source path"))?;
            let child = if let Some(child) = job.view.children.get(index) {
                *child
            } else {
                // Recover the narrow crash window after enqueue's durable paused
                // child insertion but before the parent's identity checkpoint.
                let existing = self
                    .snapshot()
                    .await
                    .into_iter()
                    .find(|d| d.destination == destination);
                let child = if let Some(existing) = existing {
                    if existing.url != source {
                        return Err(invalid(
                            "media child reservation conflicts with persisted plan",
                        ));
                    }
                    existing.id
                } else {
                    self.enqueue_paused(DownloadRequest {
                        url: source.clone(),
                        destination: destination.clone(),
                        expected_sha256: None,
                    })
                    .await?
                };
                self.inner.media_children.lock().await.insert(child);
                let mut data = self.inner.data.lock().await;
                let saved = data
                    .media_jobs
                    .iter_mut()
                    .find(|j| j.view.id == id)
                    .ok_or(Error::NotFound)?;
                saved.view.children.push(child);
                // The child is durable and paused before its identity is linked;
                // only after the parent checkpoint may any body be requested.
                self.inner.store.save_media_job(saved).await?;
                job = saved.clone();
                child
            };
            check_cancel(self, token)?;
            let state = self
                .snapshot()
                .await
                .into_iter()
                .find(|d| d.id == child)
                .ok_or(Error::NotFound)?
                .state;
            if state != DownloadState::Completed {
                self.resume(child).await?;
            }
            loop {
                check_cancel(self, token)?;
                let download = self
                    .snapshot()
                    .await
                    .into_iter()
                    .find(|d| d.id == child)
                    .ok_or(Error::NotFound)?;
                if download.url != source || download.destination != destination {
                    return Err(invalid("media segment identity changed"));
                }
                match download.state {
                    DownloadState::Completed => {
                        let hash =
                            hash_file(&download.destination, token, &self.inner.media.shutdown)
                                .await?;
                        if download.sha256.as_deref() != Some(&hash.0) {
                            return Err(Error::Integrity);
                        }
                        validate_segment(
                            &download.destination,
                            container,
                            token,
                            &self.inner.media.shutdown,
                        )
                        .await?;
                        break;
                    }
                    DownloadState::Failed | DownloadState::Canceled | DownloadState::Paused => {
                        return Err(invalid(
                            "media segment failed or was stopped; resume the media job to retry",
                        ));
                    }
                    _ => {}
                }
                tokio::select! {
                    _ = token.cancelled() => return Err(Error::Interrupted),
                    _ = self.inner.media.shutdown.cancelled() => return Err(Error::Shutdown),
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {}
                }
            }
        }
        check_cancel(self, token)?;
        owned_directory(&job).await?;
        self.verify_media_inputs(&job, token).await?;
        // Remux and full-decode stages have a separate small resource budget.
        // Waiting never bypasses pause/cancel/shutdown or occupies a connection.
        let _processing = self.acquire_media_processor(token).await?;
        let attempt = uuid::Uuid::new_v4();
        let artifact = job.view.destination.parent().unwrap().join(format!(
            ".osdm-media-{id}-{attempt}.part.{}",
            job.view
                .destination
                .extension()
                .and_then(|extension| extension.to_str())
                .ok_or_else(|| invalid("invalid output container"))?
        ));
        {
            let mut data = self.inner.data.lock().await;
            check_cancel(self, token)?;
            let saved = data
                .media_jobs
                .iter_mut()
                .find(|j| j.view.id == id)
                .ok_or(Error::NotFound)?;
            saved.artifact = Some(artifact.clone());
            saved.view.state = MediaJobState::Merging;
            self.inner.store.save_media_job(saved).await?;
        }
        let subtitle_path = if let Some(subtitle) = &job.subtitle {
            crate::media_capture::validate_subtitles(subtitle.format, &subtitle.body)?;
            let extension = if subtitle.format == crate::media_capture::SubtitleFormat::Srt {
                "srt"
            } else {
                "vtt"
            };
            let path = job.directory.join(format!("{attempt}.{extension}"));
            let mut file = tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .await?;
            file.write_all(&subtitle.body).await?;
            file.flush().await?;
            file.sync_all().await?;
            Some(path)
        } else {
            None
        };
        let mut tracks = Vec::new();
        let mut flat = 0usize;
        for track in &job.tracks {
            let files: Vec<_> = (flat..flat + track.sources.len())
                .map(|index| {
                    job.child_path(index)
                        .ok_or_else(|| invalid("media plan is missing a source path"))
                })
                .collect::<Result<_>>()?;
            flat += track.sources.len();
            tracks.push(crate::media_capture::LocalTrack {
                files,
                container: match track.container {
                    TrackContainer::MpegTs => crate::media_capture::SegmentContainer::MpegTs,
                    TrackContainer::FragmentedMp4 => {
                        crate::media_capture::SegmentContainer::FragmentedMp4
                    }
                },
            });
        }
        let mut input_bytes = 0u64;
        for track in &tracks {
            for path in &track.files {
                input_bytes = input_bytes
                    .checked_add(tokio::fs::metadata(path).await?.len())
                    .ok_or(Error::Length)?;
            }
        }
        if let Some(path) = &subtitle_path {
            input_bytes = input_bytes
                .checked_add(tokio::fs::metadata(path).await?.len())
                .ok_or(Error::Length)?;
        }
        let subtitles = subtitle_path
            .into_iter()
            .map(|path| crate::media_capture::LocalSubtitle {
                path,
                format: job.subtitle.as_ref().unwrap().format,
                language: None,
            })
            .collect();
        let plan = crate::media_capture::MergePlan {
            tracks,
            subtitles,
            output: if job
                .view
                .destination
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("mp4")
            {
                crate::media_capture::OutputContainer::Mp4
            } else {
                crate::media_capture::OutputContainer::Matroska
            },
            max_input_bytes: input_bytes,
        };
        let processing = token.child_token();
        let remux = crate::media_capture::remux_local(&job.ffmpeg, &plan, &processing);
        tokio::pin!(remux);
        let merged = tokio::select! {
            result = &mut remux => result.map_err(|error| match error {
                crate::media_capture::MergeError::Cancelled => Error::Interrupted,
                _ => invalid("configured media remuxer rejected the local VOD"),
            })?,
            _ = self.inner.media.shutdown.cancelled() => {
                processing.cancel(); let _ = remux.await; return Err(Error::Shutdown);
            }
            _ = tokio::time::sleep(Duration::from_secs(3600)) => {
                processing.cancel(); let _ = remux.await; return Err(invalid("media remux exceeded its time budget"));
            }
        };
        check_cancel(self, token)?;
        // Core artifacts are ephemeral and may be on another volume. Import with
        // bounded reads into the already-recorded same-directory create-new name.
        let mut input = tokio::fs::File::open(merged.path()).await?;
        let mut output = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&artifact)
            .await?;
        let mut buffer = vec![0; 256 * 1024];
        let mut remaining = input_bytes
            .saturating_mul(2)
            .saturating_add(16 * 1024 * 1024);
        loop {
            let count = tokio::select! {
                _ = token.cancelled() => return Err(Error::Interrupted),
                _ = self.inner.media.shutdown.cancelled() => return Err(Error::Shutdown),
                result = input.read(&mut buffer) => result?,
            };
            if count == 0 {
                break;
            }
            remaining = remaining.checked_sub(count as u64).ok_or(Error::Length)?;
            tokio::select! {
                _ = token.cancelled() => return Err(Error::Interrupted),
                _ = self.inner.media.shutdown.cancelled() => return Err(Error::Shutdown),
                result = output.write_all(&buffer[..count]) => result?,
            }
        }
        output.flush().await?;
        output.sync_all().await?;
        drop(output);
        drop(input);
        drop(merged);
        // Decode the entire remux, not just its headers. Invalid media must never
        // become 'verified' merely because arbitrary bytes were concatenated.
        let mut verify = tokio::process::Command::new(&job.ffmpeg);
        verify
            .current_dir(artifact.parent().unwrap())
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-max_alloc",
                "67108864",
                "-threads",
                "1",
                "-xerror",
                "-protocol_whitelist",
                "file",
                "-i",
            ])
            .arg(artifact.file_name().unwrap())
            .args(["-map", "0:v?", "-map", "0:a?", "-f", "null", "-"]);
        run_local(verify, token, &self.inner.media.shutdown).await?;
        // Reject inputs modified while later segments downloaded or during the
        // remux. A valid container alone does not establish plan identity.
        self.verify_media_inputs(&job, token).await?;
        let file = tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&artifact)
            .await?;
        file.sync_all().await?;
        drop(file);
        let (hash, length) = hash_file(&artifact, token, &self.inner.media.shutdown).await?;
        if length == 0 {
            return Err(invalid("ffmpeg produced an empty media artifact"));
        }
        {
            let mut data = self.inner.data.lock().await;
            check_cancel(self, token)?;
            let saved = data
                .media_jobs
                .iter_mut()
                .find(|j| j.view.id == id)
                .ok_or(Error::NotFound)?;
            saved.view.sha256 = Some(hash);
            saved.length = Some(length);
            saved.ready = true;
            saved.view.state = MediaJobState::Publishing;
            self.inner.store.save_media_job(saved).await?;
        }
        self.publish_media_job(id, token).await
    }

    async fn verify_media_inputs(&self, job: &SavedJob, token: &CancellationToken) -> Result<()> {
        let downloads = self.snapshot().await;
        for (index, child) in job.view.children.iter().enumerate() {
            check_cancel(self, token)?;
            let download = downloads
                .iter()
                .find(|d| d.id == *child)
                .ok_or(Error::NotFound)?;
            let (source, _) = job
                .source_at(index)
                .ok_or_else(|| invalid("media plan is missing a source"))?;
            let destination = job
                .child_path(index)
                .ok_or_else(|| invalid("media plan is missing a source path"))?;
            if download.state != DownloadState::Completed
                || download.url != source
                || download.destination != destination
            {
                return Err(invalid("media segment identity changed"));
            }
            let (hash, _) =
                hash_file(&download.destination, token, &self.inner.media.shutdown).await?;
            if download.sha256.as_ref() != Some(&hash) {
                return Err(Error::Integrity);
            }
        }
        Ok(())
    }

    async fn publish_media_job(&self, id: MediaJobId, token: &CancellationToken) -> Result<()> {
        let job = self.saved_media_job(id).await?;
        validate_layout(&job)?;
        let artifact = job
            .artifact
            .as_ref()
            .ok_or_else(|| invalid("missing durable media artifact"))?;
        let (hash, length) = hash_file(artifact, token, &self.inner.media.shutdown).await?;
        if Some(&hash) != job.view.sha256.as_ref() || Some(length) != job.length {
            return Err(Error::Integrity);
        }
        check_cancel(self, token)?;
        // Serialize stop against publication; a successful pause cannot race a
        // later hard link. Retain the artifact as the recovery identity anchor.
        let mut data = self.inner.data.lock().await;
        let saved = data
            .media_jobs
            .iter_mut()
            .find(|j| j.view.id == id)
            .ok_or(Error::NotFound)?;
        check_cancel(self, token)?;
        if matches!(
            saved.view.state,
            MediaJobState::Paused | MediaJobState::Canceled
        ) {
            return Err(Error::Interrupted);
        }
        match tokio::fs::hard_link(artifact, &job.view.destination).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                regular_file(&job.view.destination).await?;
                if !same_file::is_same_file(artifact, &job.view.destination)? {
                    return Err(invalid(
                        "media destination already exists; no file was overwritten",
                    ));
                }
            }
            Err(error) => return Err(error.into()),
        }
        drop(data);
        let actual = hash_file(&job.view.destination, token, &self.inner.media.shutdown).await?;
        if actual != (hash, length) || !same_file::is_same_file(artifact, &job.view.destination)? {
            return Err(Error::Integrity);
        }
        let mut data = self.inner.data.lock().await;
        let saved = data
            .media_jobs
            .iter_mut()
            .find(|j| j.view.id == id)
            .ok_or(Error::NotFound)?;
        check_cancel(self, token)?;
        if matches!(
            saved.view.state,
            MediaJobState::Paused | MediaJobState::Canceled
        ) {
            return Err(Error::Interrupted);
        }
        saved.view.state = MediaJobState::Completed;
        saved.view.completed_segments = saved.view.total_segments;
        saved.view.error = None;
        self.inner.store.save_media_job(saved).await?;
        Ok(())
    }
}

fn check_cancel(engine: &Engine, token: &CancellationToken) -> Result<()> {
    if engine.inner.media.shutdown.is_cancelled() {
        Err(Error::Shutdown)
    } else if token.is_cancelled() {
        Err(Error::Interrupted)
    } else {
        Ok(())
    }
}

fn validate_layout(job: &SavedJob) -> Result<()> {
    let parent = job
        .view
        .destination
        .parent()
        .ok_or_else(|| invalid("invalid media layout"))?;
    let total = job.total_sources();
    if !parent.is_absolute()
        || !matches!(
            job.view
                .destination
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("mkv" | "mp4")
        )
        || job.directory != parent.join(format!(".osdm-media-{}", job.view.id))
        || job.tracks.is_empty()
        || job.tracks.len() > 2
        || job.tracks.iter().any(|track| track.sources.is_empty())
        || total == 0
        || total > crate::media_capture::MAX_ENTRIES
        || job.view.children.len() > total
        || job.marker.len() != 36
    {
        return Err(invalid("invalid media ownership record"));
    }
    if let Some(path) = &job.artifact {
        let prefix = format!(".osdm-media-{}-", job.view.id);
        if path.parent() != Some(parent)
            || !path.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
                s.starts_with(&prefix)
                    && job
                        .view
                        .destination
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| s.ends_with(&format!(".part.{extension}")))
            })
        {
            return Err(invalid("invalid media artifact ownership record"));
        }
    }
    Ok(())
}

async fn owned_directory(job: &SavedJob) -> Result<()> {
    match tokio::fs::create_dir(&job.directory).await {
        Ok(()) => {
            let mut marker = tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(job.directory.join("owner"))
                .await?;
            marker.write_all(job.marker.as_bytes()).await?;
            marker.flush().await?;
            marker.sync_all().await?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let metadata = tokio::fs::symlink_metadata(&job.directory).await?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || tokio::fs::canonicalize(&job.directory).await? != job.directory
    {
        return Err(invalid(
            "media working directory ownership could not be validated",
        ));
    }
    let marker = job.directory.join("owner");
    regular_file(&marker).await?;
    if tokio::fs::metadata(&marker).await?.len() != job.marker.len() as u64
        || tokio::fs::read(&marker).await? != job.marker.as_bytes()
    {
        return Err(invalid("media working directory marker does not match"));
    }
    Ok(())
}

async fn regular_file(path: &Path) -> Result<()> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("media input is not an owned regular file"));
    }
    Ok(())
}

async fn hash_file(
    path: &Path,
    token: &CancellationToken,
    shutdown: &CancellationToken,
) -> Result<(String, u64)> {
    regular_file(path).await?;
    let mut file = tokio::fs::File::open(path).await?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    let mut length = 0;
    loop {
        let count = tokio::select! {
            _ = token.cancelled() => return Err(Error::Interrupted),
            _ = shutdown.cancelled() => return Err(Error::Shutdown),
            read = file.read(&mut buffer) => read?,
        };
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        length += count as u64;
        tokio::task::yield_now().await;
    }
    Ok((format!("{:x}", hash.finalize()), length))
}

async fn validate_segment(
    path: &Path,
    container: TrackContainer,
    token: &CancellationToken,
    shutdown: &CancellationToken,
) -> Result<()> {
    match container {
        TrackContainer::MpegTs => validate_ts(path, token, shutdown).await,
        TrackContainer::FragmentedMp4 => validate_fmp4(path, token, shutdown).await,
    }
}

/// A conservative ISO-BMFF shape check. Full decode still happens after remux.
async fn validate_fmp4(
    path: &Path,
    token: &CancellationToken,
    shutdown: &CancellationToken,
) -> Result<()> {
    let mut file = tokio::fs::File::open(path).await?;
    let length = file.metadata().await?.len();
    if length < 8 {
        return Err(invalid("invalid fragmented MP4 segment"));
    }
    let mut header = [0u8; 8];
    tokio::select! {
        _ = token.cancelled() => return Err(Error::Interrupted),
        _ = shutdown.cancelled() => return Err(Error::Shutdown),
        result = file.read_exact(&mut header) => { result?; }
    }
    let size = u32::from_be_bytes(header[..4].try_into().expect("four bytes"));
    if size < 8
        || !matches!(
            &header[4..8],
            b"ftyp" | b"styp" | b"sidx" | b"moof" | b"moov" | b"mdat"
        )
    {
        return Err(invalid("invalid or unsupported fragmented MP4 segment"));
    }
    Ok(())
}

async fn validate_ts(
    path: &Path,
    token: &CancellationToken,
    shutdown: &CancellationToken,
) -> Result<()> {
    let mut file = tokio::fs::File::open(path).await?;
    let length = file.metadata().await?.len();
    if length == 0 || length % 188 != 0 {
        return Err(invalid(
            "only standalone 188-byte MPEG-TS HLS segments are supported",
        ));
    }
    let mut buffer = vec![0; 188 * 256];
    let mut remaining = length;
    while remaining > 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        tokio::select! {
            _ = token.cancelled() => return Err(Error::Interrupted),
            _ = shutdown.cancelled() => return Err(Error::Shutdown),
            result = file.read_exact(&mut buffer[..count]) => { result?; }
        }
        if buffer[..count]
            .as_chunks::<188>()
            .0
            .iter()
            .any(|packet| packet[0] != 0x47 || packet[1] & 0x80 != 0)
        {
            return Err(invalid("invalid or unsupported MPEG-TS media segment"));
        }
        remaining -= count as u64;
        tokio::task::yield_now().await;
    }
    Ok(())
}

async fn run_local(
    mut command: tokio::process::Command,
    token: &CancellationToken,
    shutdown: &CancellationToken,
) -> Result<()> {
    // Never inherit terminal stdin or retain unbounded ffmpeg diagnostics.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| invalid("could not launch configured ffmpeg executable"))?;
    tokio::select! {
        result = child.wait() => {
            if !result?.success() { return Err(invalid("ffmpeg rejected media or failed to remux/validate it")); }
            Ok(())
        }
        _ = token.cancelled() => { let _ = child.kill().await; let _ = child.wait().await; Err(Error::Interrupted) }
        _ = shutdown.cancelled() => { let _ = child.kill().await; let _ = child.wait().await; Err(Error::Shutdown) }
        _ = tokio::time::sleep(Duration::from_secs(3600)) => {
            let _ = child.kill().await; let _ = child.wait().await;
            Err(invalid("local media processing exceeded its time budget"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_subprocess_cancellation_kills_and_waits_for_real_ffmpeg() {
        let ffmpeg = std::env::var_os("OSDM_TEST_FFMPEG")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                    .map(|directory| {
                        directory.join(if cfg!(windows) {
                            "ffmpeg.exe"
                        } else {
                            "ffmpeg"
                        })
                    })
                    .find(|path| path.is_file())
            });
        let Some(ffmpeg) = ffmpeg else {
            return;
        };
        let mut command = tokio::process::Command::new(ffmpeg);
        command.args([
            "-nostdin",
            "-loglevel",
            "error",
            "-re",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x64:rate=10",
            "-t",
            "60",
            "-f",
            "null",
            "-",
        ]);
        let token = CancellationToken::new();
        let shutdown = CancellationToken::new();
        let operation = run_local(command, &token, &shutdown);
        tokio::pin!(operation);
        tokio::select! {
            _ = &mut operation => panic!("real-time ffmpeg must remain active until cancelled"),
            _ = tokio::time::sleep(Duration::from_millis(150)) => {}
        }
        token.cancel();
        let result = tokio::time::timeout(Duration::from_secs(3), operation)
            .await
            .unwrap();
        assert!(matches!(result, Err(Error::Interrupted)));
    }

    #[test]
    fn request_debug_never_exposes_source_or_subtitle_urls() {
        let request = MediaJobRequest {
            url: "https://source.invalid/private?token=secret".into(),
            destination: PathBuf::from("output.mkv"),
            ffmpeg_path: PathBuf::from("ffmpeg"),
            allowed_origins: vec![],
            description: None,
            category_id: None,
            subtitle_url: Some("https://source.invalid/private-subtitle?token=secret".into()),
            dash_tracks: vec!["video-representation".into()],
            direct_media: false,
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("source.invalid"));
        assert!(!debug.contains("secret"));
    }

    #[tokio::test]
    async fn media_processing_admission_is_bounded_and_cancellable() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path().join("processing.db"), crate::Config::default())
            .await
            .unwrap();
        let token = CancellationToken::new();
        let first = engine.acquire_media_processor(&token).await.unwrap();
        let second = engine.acquire_media_processor(&token).await.unwrap();
        let waiting = engine.acquire_media_processor(&token);
        tokio::pin!(waiting);
        tokio::select! {
            biased;
            _ = &mut waiting => panic!("processing must respect its two-stage ceiling"),
            _ = tokio::task::yield_now() => {}
        }
        token.cancel();
        assert!(matches!(waiting.await, Err(Error::Interrupted)));
        drop(first);
        drop(second);
        engine.shutdown().await.unwrap();
    }
}
