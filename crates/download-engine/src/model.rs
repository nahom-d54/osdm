use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

pub type DownloadId = Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub max_active_downloads: usize,
    pub connections_per_download: usize,
    pub max_connections: usize,
    pub max_connections_per_host: usize,
    pub split_threshold: u64,
    /// Zero means unlimited. Applies to all downloads together.
    pub bytes_per_second: u64,
    pub max_retries: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_active_downloads: 3,
            connections_per_download: 4,
            max_connections: 12,
            max_connections_per_host: 8,
            split_threshold: 16 * 1024 * 1024,
            bytes_per_second: 0,
            max_retries: 3,
        }
    }
}

impl Config {
    pub(crate) fn validate(&self) -> crate::Result<()> {
        if !(1..=32).contains(&self.max_active_downloads)
            || !(1..=8).contains(&self.connections_per_download)
            || !(1..=256).contains(&self.max_connections)
            || !(1..=64).contains(&self.max_connections_per_host)
            || self.max_retries > 10
        {
            return Err(crate::Error::Invalid(
                "invalid concurrency or retry limits".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub url: String,
    /// Absolute destination filename; existing files are never overwritten.
    pub destination: PathBuf,
    /// SHA-256 supplied by the publisher/user. Hexadecimal, exactly 64 digits.
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

/// Explicit per-transfer Basic authentication. Session-only; never serialized,
/// logged, included in a Download snapshot, or stored in SQLite.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasicCredentials {
    pub username: String,
    pub password: String,
}

/// Durable non-secret organization metadata. Credentials never belong here.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DownloadProperties {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category_id: Option<String>,
}

impl DownloadProperties {
    pub(crate) fn validate(&self) -> crate::Result<()> {
        if self.description.len() > 4096
            || self
                .category_id
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 128)
        {
            return Err(crate::Error::Invalid("invalid download properties".into()));
        }
        Ok(())
    }
}

/// Metadata read from a URL before it is enqueued. No queue entry is created.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeInfo {
    /// URL after redirects.
    pub final_url: String,
    /// Filename supplied by Content-Disposition, with path components removed.
    /// The desktop workflow still sanitizes this for the target filesystem.
    pub filename: Option<String>,
    /// MIME type before any `; charset=...` parameter.
    pub content_type: Option<String>,
    /// Total resource length when the server reports one.
    pub total_bytes: Option<u64>,
    /// True when the server honored a one-byte range request.
    pub ranged: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadState {
    Queued,
    Downloading,
    Pausing,
    Paused,
    Canceling,
    Canceled,
    Failed,
    Completed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    /// Exclusive end. Unknown only for a single streaming transfer.
    pub end: Option<u64>,
    /// Bytes known to have been synced to disk.
    pub committed: u64,
    /// Live bytes; reset to committed after recovery.
    pub downloaded: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Download {
    pub id: DownloadId,
    pub url: String,
    pub destination: PathBuf,
    #[serde(default)]
    pub properties: DownloadProperties,
    pub temporary_path: PathBuf,
    /// Canonical user-selected working directory, constrained to the destination volume.
    #[serde(default)]
    pub temporary_directory: Option<PathBuf>,
    pub state: DownloadState,
    pub total_bytes: Option<u64>,
    pub segments: Vec<Segment>,
    pub error: Option<String>,
    pub bytes_per_second: u64,
    pub eta_seconds: Option<u64>,
    /// Runtime feedback, not recovered from a previous process.
    #[serde(default, skip_deserializing)]
    pub adaptive: Option<AdaptiveStatus>,
    #[serde(default)]
    pub expected_sha256: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub integrity: IntegrityStatus,
    pub(crate) validator: Option<String>,
    pub(crate) ranged: bool,
    /// Durable intent used to recover a crash during final publication.
    pub(crate) ready_to_publish: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrityStatus {
    #[default]
    Unverified,
    Verifying,
    /// A hash was computed, but no trusted reference was supplied.
    Computed,
    Verified,
    Mismatch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdaptiveStatus {
    pub active_connections: usize,
    pub host_connections: usize,
    pub host_limit: usize,
    pub host_ceiling: usize,
    pub chunk_bytes: u64,
    pub reason: String,
}

impl Download {
    pub fn downloaded_bytes(&self) -> u64 {
        self.segments.iter().map(|s| s.downloaded).sum()
    }
}

#[derive(Clone, Debug)]
pub enum EngineEvent {
    /// Full snapshot, at most four times per second. On lag, call snapshot().
    Snapshot(Vec<Download>),
    /// Persistence or scheduler failure; URL credentials are not included.
    Error(String),
}
