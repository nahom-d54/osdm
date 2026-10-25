//! Optional same-volume working storage. Final publication always links from
//! a same-directory staging name; no cross-volume copy or overwrite fallback.
use crate::{Download, Error, Result};
use std::path::{Path, PathBuf};

pub(crate) fn publication_source(download: &Download) -> PathBuf {
    if download.temporary_directory.is_some() {
        download
            .destination
            .parent()
            .unwrap()
            .join(format!(".osdm-{}.publish.part", download.id))
    } else {
        download.temporary_path.clone()
    }
}

pub(crate) async fn validate_directory(
    directory: PathBuf,
    destination_parent: &Path,
) -> Result<PathBuf> {
    if !directory.is_absolute() {
        return Err(Error::Invalid(
            "temporary directory must be absolute".into(),
        ));
    }
    let directory = tokio::fs::canonicalize(directory).await?;
    let name = format!(".osdm-storage-check-{}", uuid::Uuid::new_v4());
    let source = directory.join(&name);
    let target = destination_parent.join(&name);
    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&source)
        .await?;
    drop(file);
    let result = if source == target {
        Ok(())
    } else {
        tokio::fs::hard_link(&source, &target).await
    };
    // Only names created by this invocation may be unlinked. In particular do
    // not remove target when hard_link reports an existing foreign file.
    if source != target && result.is_ok() {
        tokio::fs::remove_file(&target).await?;
    }
    tokio::fs::remove_file(&source).await?;
    result.map_err(|_| {
        Error::Invalid("temporary storage must support hard links on the destination volume".into())
    })?;
    Ok(directory)
}
