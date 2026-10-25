//! Final-file hashing uses bounded memory and is cancellable like a transfer.
use crate::{engine::Inner, *};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

pub(crate) fn normalize(value: Option<String>) -> Result<Option<String>> {
    value
        .map(|value| {
            let value = value.trim();
            if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(Error::Invalid(
                    "SHA-256 must contain exactly 64 hexadecimal characters".into(),
                ));
            }
            Ok(value.to_ascii_lowercase())
        })
        .transpose()
}

pub(crate) async fn verify(inner: &Inner, id: DownloadId, token: &CancellationToken) -> Result<()> {
    let (path, expected, length) = {
        let mut data = inner.data.lock().await;
        let download = data
            .downloads
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(Error::NotFound)?;
        download.integrity = IntegrityStatus::Verifying;
        download.sha256 = None;
        (
            download.temporary_path.clone(),
            download.expected_sha256.clone(),
            download.downloaded_bytes(),
        )
    };
    let result = async {
        let mut file = tokio::fs::File::open(path).await?;
        if file.metadata().await?.len() != length {
            return Err(Error::Length);
        }
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 256 * 1024];
        let mut read = 0u64;
        loop {
            let count = tokio::select! {
                _ = token.cancelled() => return Err(Error::Interrupted),
                result = file.read(&mut buffer) => result?,
            };
            if count == 0 {
                break;
            }
            read += count as u64;
            hasher.update(&buffer[..count]);
            #[cfg(debug_assertions)]
            if read == count as u64 {
                tokio::select! {
                    _ = token.cancelled() => return Err(Error::Interrupted),
                    _ = crate::test_faults::hashing(id) => {}
                }
            }
            tokio::task::yield_now().await;
        }
        if read != length {
            return Err(Error::Length);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }
    .await;
    let mut data = inner.data.lock().await;
    let download = data
        .downloads
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or(Error::NotFound)?;
    match result {
        Ok(actual) => {
            let matches = expected.as_ref().is_none_or(|expected| *expected == actual);
            download.sha256 = Some(actual);
            download.integrity = if !matches {
                IntegrityStatus::Mismatch
            } else if expected.is_some() {
                IntegrityStatus::Verified
            } else {
                IntegrityStatus::Computed
            };
            inner.save(&data, id).await?;
            if matches {
                Ok(())
            } else {
                Err(Error::Integrity)
            }
        }
        Err(error) => {
            download.integrity = IntegrityStatus::Unverified;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_sha256_and_rejects_invalid_values() {
        let upper = "A".repeat(64);
        assert_eq!(normalize(Some(upper)).unwrap(), Some("a".repeat(64)));
        assert_eq!(normalize(None).unwrap(), None);
        for invalid in [String::new(), "a".repeat(63), "g".repeat(64)] {
            assert!(
                normalize(Some(invalid.clone())).is_err(),
                "accepted {invalid:?}"
            );
        }
    }
}
