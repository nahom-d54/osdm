use crate::{Download, DownloadId, DownloadState, Engine, Error, Result};

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct OwnedPartial {
    pub id: DownloadId,
    pub temporary_path: std::path::PathBuf,
    pub destination: std::path::PathBuf,
    #[serde(default)]
    pub temporary_directory: Option<std::path::PathBuf>,
}

impl From<&Download> for OwnedPartial {
    fn from(download: &Download) -> Self {
        Self {
            id: download.id,
            temporary_path: download.temporary_path.clone(),
            destination: download.destination.clone(),
            temporary_directory: download.temporary_directory.clone(),
        }
    }
}

fn terminal(state: DownloadState) -> bool {
    matches!(
        state,
        DownloadState::Failed | DownloadState::Canceled | DownloadState::Completed
    )
}

/// Validate ownership from the persisted UUID/path, never from a directory glob.
async fn remove_owned_partial(download: &OwnedPartial) -> Result<usize> {
    let parent = download
        .destination
        .parent()
        .ok_or_else(|| Error::Invalid("invalid destination".into()))?;
    let working_parent = download.temporary_directory.as_deref().unwrap_or(parent);
    let expected = working_parent.join(format!(".osdm-{}.part", download.id));
    if !expected.is_absolute()
        || expected != download.temporary_path
        || expected == download.destination
    {
        return Err(Error::Invalid(
            "refusing to remove an unowned temporary path".into(),
        ));
    }
    let mut removed = 0;
    if download.temporary_directory.is_some() {
        let staging = parent.join(format!(".osdm-{}.publish.part", download.id));
        if staging == download.destination {
            return Err(Error::Invalid("invalid publication staging path".into()));
        }
        if tokio::fs::symlink_metadata(&staging).await.is_ok()
            && !same_file::is_same_file(&staging, &expected).unwrap_or(false)
            && !same_file::is_same_file(&staging, &download.destination).unwrap_or(false)
        {
            return Err(Error::Invalid(
                "refusing to remove an unrelated publication staging file".into(),
            ));
        }
        removed += remove_regular_owned_name(&staging, parent).await?;
    }
    removed += remove_regular_owned_name(&expected, working_parent).await?;
    Ok(removed)
}

async fn remove_regular_owned_name(
    expected: &std::path::Path,
    parent: &std::path::Path,
) -> Result<usize> {
    let metadata = match tokio::fs::symlink_metadata(expected).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(Error::Invalid(
            "refusing to remove a non-regular temporary file".into(),
        ));
    }
    if tokio::fs::canonicalize(parent).await? != parent {
        return Err(Error::Invalid(
            "temporary file parent no longer matches its recorded directory".into(),
        ));
    }
    // Unlink only this name. If publication created a hard link, the destination
    // remains intact; never canonicalize a file path into its symlink target.
    tokio::fs::remove_file(expected).await?;
    Ok(1)
}

impl Engine {
    /// Remove terminal history. Partial data is either explicitly deleted or recorded
    /// in the orphan ledger for a later explicit cleanup. Remaining queue order is unchanged.
    pub async fn remove_download(&self, id: DownloadId, remove_partial_file: bool) -> Result<()> {
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        let index = data
            .downloads
            .iter()
            .position(|d| d.id == id)
            .ok_or(Error::NotFound)?;
        let download = &data.downloads[index];
        if data.automation.membership.get(&id).is_some_and(|queue| {
            data.automation
                .queues
                .iter()
                .any(|q| q.id == *queue && q.synchronize)
        }) {
            return Err(Error::Invalid(
                "remove the entry from its synchronization queue before deleting history".into(),
            ));
        }
        if data.running.contains_key(&id) || !terminal(download.state) {
            return Err(Error::Invalid(
                "only fully stopped terminal downloads can be removed".into(),
            ));
        }
        if remove_partial_file {
            if download.state != DownloadState::Completed {
                let mut reset = download.clone();
                reset.segments.clear();
                reset.ready_to_publish = false;
                reset.ranged = false;
                reset.validator = None;
                self.inner.store.save(&reset, index).await?;
                data.downloads[index] = reset;
            }
            remove_owned_partial(&OwnedPartial::from(&data.downloads[index])).await?;
        }
        let download = &data.downloads[index];
        let retain_partial = !remove_partial_file
            && (tokio::fs::try_exists(&download.temporary_path).await?
                || tokio::fs::try_exists(crate::storage::publication_source(download)).await?);
        self.inner.store.remove(download, retain_partial).await?;
        data.downloads.remove(index);
        self.inner.credentials.lock().await.remove(&id);
        self.inner.captured_headers.lock().await.remove(&id);
        Ok(())
    }

    /// Explicit cleanup of failed/canceled partials and previously removed history.
    /// Does not scan directories or touch queued, paused, active, or published files.
    pub async fn cleanup_partial_data(&self) -> Result<usize> {
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        let mut removed = 0;
        for index in 0..data.downloads.len() {
            let download = &data.downloads[index];
            if !matches!(
                download.state,
                DownloadState::Failed | DownloadState::Canceled
            ) || data.running.contains_key(&download.id)
            {
                continue;
            }
            // Invalidate recovery intent before deletion, so a crash cannot leave a
            // ready-to-publish record referring to a partial file we just removed.
            let mut reset = download.clone();
            reset.segments.clear();
            reset.ready_to_publish = false;
            reset.ranged = false;
            reset.validator = None;
            self.inner.store.save(&reset, index).await?;
            data.downloads[index] = reset;
            removed += remove_owned_partial(&OwnedPartial::from(&data.downloads[index])).await?;
        }
        for orphan in self.inner.store.orphans().await? {
            if data
                .downloads
                .iter()
                .any(|d| d.id == orphan.id || d.temporary_path == orphan.temporary_path)
            {
                return Err(Error::Invalid(
                    "orphan ledger conflicts with an existing download".into(),
                ));
            }
            removed += remove_owned_partial(&orphan).await?;
            self.inner.store.forget_orphan(orphan.id).await?;
        }
        Ok(removed)
    }
}
