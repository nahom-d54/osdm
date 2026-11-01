#![doc = include_str!("../README.md")]
mod adaptive;
mod automation;
mod cleanup;
mod dash_inspection;
mod engine;
mod integrity;
mod media;
mod media_jobs;
mod model;
mod retry;
mod site_jobs;
#[cfg(test)]
mod site_transfer_tests;
mod storage;
mod store;
#[cfg(debug_assertions)]
#[doc(hidden)]
pub mod test_faults;
mod transfer;

pub use automation::*;
pub use dash_inspection::{dash_track_key, select_dash_tracks};
pub use engine::Engine;
pub use media::SitePageResponse;
pub use media_jobs::{MediaJob, MediaJobId, MediaJobRequest, MediaJobState};
pub use model::*;
pub use osdm_media_capture as media_capture;
pub use site_jobs::{SiteAction, SiteProject, SiteProjectRequest, SiteProjectState, SiteResource};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Media(#[from] osdm_media_capture::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("download not found")]
    NotFound,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("invalid persisted data: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("network request failed")]
    Network(#[source] reqwest::Error),
    #[error("HTTP status {0}")]
    Http(u16),
    #[error("server returned an invalid byte range or changed the resource")]
    Range,
    #[error("response length does not match the expected size")]
    Length,
    #[error(
        "SHA-256 checksum mismatch; the file was not published. Retry to download a fresh copy"
    )]
    Integrity,
    #[error("operation interrupted")]
    Interrupted,
    #[doc(hidden)]
    #[error("connection rescheduled by adaptive controller")]
    Rebalance,
    #[error("engine is shut down")]
    Shutdown,
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error.without_url())
    }
}
