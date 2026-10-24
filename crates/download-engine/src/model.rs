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

#[derive(Clone, Debug)]
pub struct DownloadRequest {
    pub url: String,
    /// Absolute destination filename; existing files are never overwritten.
    pub destination: PathBuf,
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
    pub temporary_path: PathBuf,
    pub state: DownloadState,
    pub total_bytes: Option<u64>,
    pub segments: Vec<Segment>,
    pub error: Option<String>,
    pub bytes_per_second: u64,
    pub eta_seconds: Option<u64>,
    pub(crate) validator: Option<String>,
    pub(crate) ranged: bool,
    /// Durable intent used to recover a crash during final publication.
    pub(crate) ready_to_publish: bool,
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
