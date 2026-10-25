use crate::{engine::Inner, *};
use futures_util::StreamExt;
use reqwest::{Response, header};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::OwnedSemaphorePermit,
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

pub(crate) struct Probe {
    pub(crate) url: String,
    pub(crate) length: Option<u64>,
    pub(crate) ranged: bool,
    pub(crate) validator: Option<String>,
    pub(crate) filename: Option<String>,
    pub(crate) content_type: Option<String>,
}

async fn permits(
    inner: &Inner,
    url: &str,
    token: &CancellationToken,
) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit)> {
    let url = reqwest::Url::parse(url).map_err(|_| Error::Invalid("invalid URL".into()))?;
    let host = url
        .host_str()
        .ok_or_else(|| Error::Invalid("missing host".into()))?
        .to_owned();
    let host_limit = inner
        .hosts
        .lock()
        .await
        .entry(host)
        .or_insert_with(|| {
            Arc::new(tokio::sync::Semaphore::new(
                inner.config.max_connections_per_host,
            ))
        })
        .clone();
    tokio::select! {
        _ = token.cancelled() => Err(Error::Interrupted),
        permits = async {
            let host = host_limit.acquire_owned().await.map_err(|_| Error::Shutdown)?;
            let global = inner.connections.clone().acquire_owned().await.map_err(|_| Error::Shutdown)?;
            Ok((host, global))
        } => permits,
    }
}

fn content_range(response: &Response) -> Option<(u64, u64, u64)> {
    let value = response
        .headers()
        .get(header::CONTENT_RANGE)?
        .to_str()
        .ok()?
        .strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}

fn validator(response: &Response) -> Option<String> {
    response
        .headers()
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.starts_with('"') && v.ends_with('"'))
        .map(str::to_owned)
        .or_else(|| {
            response
                .headers()
                .get(header::LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        })
}

pub(crate) async fn probe(inner: &Inner, url: &str, token: &CancellationToken) -> Result<Probe> {
    let _permits = permits(inner, url, token).await?;
    let response = tokio::select! {
        _ = token.cancelled() => return Err(Error::Interrupted),
        response = inner.client.get(url).header(header::ACCEPT_ENCODING, "identity").header(header::RANGE, "bytes=0-0").send() => response?,
    };
    let url = response.url().to_string();
    let validator = validator(&response);
    let filename = content_disposition_filename(&response);
    let content_type = response_content_type(&response);
    if response.status().as_u16() == 206 {
        let (start, end, total) = content_range(&response).ok_or(Error::Range)?;
        if start != 0 || end != 0 || total == 0 {
            return Err(Error::Range);
        }
        // Do not buffer an unbounded response from a server claiming a one-byte range.
        let mut stream = response.bytes_stream();
        let mut received = 0;
        loop {
            let chunk = tokio::select! {
                _ = token.cancelled() => return Err(Error::Interrupted),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else { break };
            received += chunk?.len();
            if received > 1 {
                return Err(Error::Range);
            }
        }
        if received != 1 {
            return Err(Error::Length);
        }
        Ok(Probe {
            url,
            length: Some(total),
            ranged: true,
            validator,
            filename,
            content_type,
        })
    } else if response.status().as_u16() == 200 {
        Ok(Probe {
            url,
            length: response.content_length(),
            ranged: false,
            validator,
            filename,
            content_type,
        })
    } else if response.status().as_u16() == 416
        && response
            .headers()
            .get(header::CONTENT_RANGE)
            .is_some_and(|v| v == "bytes */0")
    {
        Ok(Probe {
            url,
            length: Some(0),
            ranged: false,
            validator,
            filename,
            content_type,
        })
    } else {
        Err(Error::Http(response.status().as_u16()))
    }
}

fn response_content_type(response: &Response) -> Option<String> {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn content_disposition_filename(response: &Response) -> Option<String> {
    response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_disposition_filename)
}

fn parse_content_disposition_filename(header: &str) -> Option<String> {
    let params = disposition_params(header);
    let extended = params
        .iter()
        .rev()
        .find(|(key, _)| key == "filename*")
        .and_then(|(_, value)| decode_extended_filename(value))
        .and_then(|value| safe_filename(&value));
    extended.or_else(|| {
        params
            .iter()
            .rev()
            .find(|(key, _)| key == "filename")
            .and_then(|(_, value)| safe_filename(value))
    })
}

fn disposition_params(header: &str) -> Vec<(String, String)> {
    let mut params = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in header.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if quoted && character == '\\' {
            current.push(character);
            escaped = true;
            continue;
        }
        match character {
            '"' => {
                quoted = !quoted;
                current.push(character);
            }
            ';' if !quoted => {
                push_disposition_param(&mut params, &current);
                current.clear();
            }
            _ => current.push(character),
        }
    }
    push_disposition_param(&mut params, &current);
    params
}

fn push_disposition_param(params: &mut Vec<(String, String)>, raw: &str) {
    let Some((key, value)) = raw.split_once('=') else {
        return;
    };
    let key = key.trim().to_ascii_lowercase();
    if key.is_empty() {
        return;
    }
    let value = value.trim();
    let value = if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        unquote(&value[1..value.len() - 1])
    } else {
        value.to_owned()
    };
    params.push((key, value));
}

fn unquote(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            if let Some(next) = characters.next() {
                output.push(next);
            }
        } else {
            output.push(character);
        }
    }
    output
}

fn decode_extended_filename(value: &str) -> Option<String> {
    let (charset, rest) = value.split_once('\'')?;
    let (_language, encoded) = rest.split_once('\'')?;
    if !charset.is_empty() && !charset.eq_ignore_ascii_case("utf-8") {
        return None;
    }
    percent_decode(encoded)
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = (*bytes.get(index + 1)? as char).to_digit(16)?;
            let low = (*bytes.get(index + 2)? as char).to_digit(16)?;
            decoded.push((high as u8) * 16 + low as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Some(String::from_utf8_lossy(&decoded).into_owned())
}

/// Server filenames are untrusted. Remove directories, control characters, and
/// trailing Windows-ambiguous dots before they reach the naming workflow.
fn safe_filename(value: &str) -> Option<String> {
    let base = value.rsplit(['/', '\\']).next().unwrap_or(value);
    let without_control: String = base
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let name = without_control
        .trim()
        .trim_end_matches([' ', '.'])
        .trim()
        .to_owned();
    if name.is_empty() || name == "." || name == ".." {
        None
    } else {
        Some(name)
    }
}

pub(crate) async fn download(
    inner: Arc<Inner>,
    id: DownloadId,
    token: CancellationToken,
) -> Result<()> {
    let original = {
        let data = inner.data.lock().await;
        data.downloads.iter().find(|d| d.id == id).unwrap().clone()
    };
    if original.ready_to_publish {
        return publish(&inner, id, &token).await;
    }
    let mut inspected = None;
    for attempt in 0..=inner.config.max_retries {
        match probe(&inner, &original.url, &token).await {
            Ok(value) => {
                inspected = Some(value);
                break;
            }
            Err(error) if retryable(&error) && attempt < inner.config.max_retries => {
                backoff(attempt, None, &token).await?;
            }
            Err(error) => return Err(error),
        }
    }
    let probe = inspected.unwrap();
    prepare(&inner, id, &probe, false).await?;
    let result = segments(inner.clone(), id, &probe.url, &token).await;
    if matches!(result, Err(Error::Range)) && !token.is_cancelled() {
        // A server can advertise ranges and subsequently ignore them. Discard all partial
        // state before retrying as one stream; never mix full responses with ranged data.
        prepare(&inner, id, &probe, true).await?;
        segments(inner.clone(), id, &probe.url, &token).await?;
    } else {
        result?;
    }
    publish(&inner, id, &token).await
}

async fn prepare(inner: &Inner, id: DownloadId, probe: &Probe, force_stream: bool) -> Result<()> {
    let mut data = inner.data.lock().await;
    let download = data.downloads.iter().find(|d| d.id == id).unwrap().clone();
    let metadata = match tokio::fs::metadata(&download.temporary_path).await {
        Ok(value) => Some(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let can_resume = !force_stream
        && probe.ranged
        && download.ranged
        && probe.validator.is_some()
        && probe.validator == download.validator
        && probe.length == download.total_bytes
        && !download.segments.is_empty()
        && metadata.as_ref().is_some_and(|m| {
            download
                .segments
                .iter()
                .all(|s| s.start + s.committed <= m.len())
        });
    if !can_resume {
        // Invalidate old durable offsets BEFORE truncation. A crash between truncation
        // and saving the new layout must never turn preallocated zeroes into progress.
        let current = data.downloads.iter_mut().find(|d| d.id == id).unwrap();
        current.segments.clear();
        current.ranged = false;
        current.validator = None;
        current.ready_to_publish = false;
        inner.save(&data, id).await?;
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true);
        if metadata.is_some() {
            options.truncate(true);
        } else {
            options.create_new(true);
        }
        let file = options.open(&download.temporary_path).await?;
        if let Some(length) = probe.length {
            file.set_len(length).await?;
        }
        file.sync_all().await?;
        let download = data.downloads.iter_mut().find(|d| d.id == id).unwrap();
        download.total_bytes = probe.length;
        download.validator = probe.validator.clone();
        // A validator is required to bind retries and parallel requests to one version.
        download.ranged = probe.ranged && probe.validator.is_some() && !force_stream;
        download.ready_to_publish = false;
        let count = if download.ranged
            && probe
                .length
                .is_some_and(|n| n >= inner.config.split_threshold && n > 0)
        {
            inner
                .config
                .connections_per_download
                .min(probe.length.unwrap() as usize)
                .max(1)
        } else {
            1
        };
        download.segments = (0..count)
            .map(|index| {
                let length = probe.length.unwrap_or(0);
                let size = length / count as u64;
                Segment {
                    start: size * index as u64,
                    end: probe.length.map(|n| {
                        if index + 1 == count {
                            n
                        } else {
                            size * (index + 1) as u64
                        }
                    }),
                    committed: 0,
                    downloaded: 0,
                }
            })
            .collect();
    }
    let download = data.downloads.iter_mut().find(|d| d.id == id).unwrap();
    for segment in &mut download.segments {
        segment.downloaded = segment.committed;
    }
    inner.save(&data, id).await
}

async fn segments(
    inner: Arc<Inner>,
    id: DownloadId,
    url: &str,
    token: &CancellationToken,
) -> Result<()> {
    let count = inner
        .data
        .lock()
        .await
        .downloads
        .iter()
        .find(|d| d.id == id)
        .unwrap()
        .segments
        .len();
    let children = token.child_token();
    let mut tasks = JoinSet::new();
    for index in 0..count {
        let inner = inner.clone();
        let token = children.clone();
        let url = url.to_owned();
        tasks.spawn(async move { segment(inner, id, index, &url, &token).await });
    }
    let mut failure = None;
    while let Some(result) = tasks.join_next().await {
        let result = result.unwrap_or_else(|_| {
            Err(Error::Invalid(
                "segment worker terminated unexpectedly".into(),
            ))
        });
        if let Err(error) = result
            && failure.is_none()
        {
            failure = Some(error);
            children.cancel();
        }
    }
    failure.map_or(Ok(()), Err)
}

async fn segment(
    inner: Arc<Inner>,
    id: DownloadId,
    index: usize,
    url: &str,
    token: &CancellationToken,
) -> Result<()> {
    for attempt in 0..=inner.config.max_retries {
        let mut retry_after = None;
        match attempt_segment(&inner, id, index, url, token, &mut retry_after).await {
            Ok(()) => return Ok(()),
            Err(error) if retryable(&error) && attempt < inner.config.max_retries => {
                backoff(attempt, retry_after, token).await?;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

fn retryable(error: &Error) -> bool {
    matches!(
        error,
        Error::Network(_) | Error::Length | Error::Http(408 | 429 | 500..=599)
    )
}

async fn backoff(attempt: u32, retry_after: Option<u64>, token: &CancellationToken) -> Result<()> {
    let delay = retry_after.unwrap_or(1 << attempt.min(5)).min(120);
    tokio::select! {
        _ = token.cancelled() => Err(Error::Interrupted),
        _ = tokio::time::sleep(Duration::from_secs(delay)) => Ok(()),
    }
}

async fn attempt_segment(
    inner: &Inner,
    id: DownloadId,
    index: usize,
    url: &str,
    token: &CancellationToken,
    retry_after: &mut Option<u64>,
) -> Result<()> {
    if token.is_cancelled() {
        return Err(Error::Interrupted);
    }
    let download = {
        let mut data = inner.data.lock().await;
        let download = data.downloads.iter_mut().find(|d| d.id == id).unwrap();
        let segment = &mut download.segments[index];
        if !download.ranged {
            segment.committed = 0;
        }
        segment.downloaded = segment.committed;
        download.clone()
    };
    let segment = &download.segments[index];
    if segment.end == Some(segment.start + segment.committed) {
        return Ok(());
    }
    let _permits = permits(inner, url, token).await?;
    let offset = segment.start + segment.committed;
    let mut request = inner
        .client
        .get(url)
        .header(header::ACCEPT_ENCODING, "identity");
    if download.ranged {
        request = request.header(
            header::RANGE,
            format!("bytes={offset}-{}", segment.end.unwrap() - 1),
        );
        if let Some(validator) = &download.validator {
            request = request.header(header::IF_RANGE, validator);
        }
    }
    let response = tokio::select! {
        _ = token.cancelled() => return Err(Error::Interrupted),
        response = request.send() => response?,
    };
    let status = response.status().as_u16();
    if status == 429 || status == 503 {
        *retry_after = response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
    }
    if status >= 400 {
        return Err(Error::Http(status));
    }
    if response
        .headers()
        .get(header::CONTENT_ENCODING)
        .is_some_and(|v| v != "identity")
    {
        return Err(Error::Invalid(
            "server returned an encoded representation despite identity encoding".into(),
        ));
    }
    if download.ranged {
        if status != 206
            || content_range(&response)
                != Some((
                    offset,
                    segment.end.unwrap() - 1,
                    download.total_bytes.unwrap(),
                ))
        {
            return Err(Error::Range);
        }
        if validator(&response).is_some_and(|v| Some(&v) != download.validator.as_ref())
            && download.validator.is_some()
        {
            return Err(Error::Range);
        }
    } else if status != 200 {
        return Err(Error::Http(status));
    }
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&download.temporary_path)
        .await?;
    if !download.ranged {
        file.set_len(download.total_bytes.unwrap_or(0)).await?;
    }
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut stream = response.bytes_stream();
    let mut received = segment.committed;
    let mut committed = received;
    let mut failure = None;
    loop {
        let chunk = tokio::select! {
            _ = token.cancelled() => { failure = Some(Error::Interrupted); break; },
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else { break };
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                failure = Some(error.into());
                break;
            }
        };
        if segment
            .end
            .is_some_and(|end| segment.start + received + chunk.len() as u64 > end)
        {
            failure = Some(Error::Length);
            break;
        }
        for bytes in chunk.chunks(64 * 1024) {
            if let Err(error) = throttle(inner, bytes.len() as u64, token).await {
                failure = Some(error);
                break;
            }
            file.write_all(bytes).await?;
            received += bytes.len() as u64;
            let mut data = inner.data.lock().await;
            data.downloads
                .iter_mut()
                .find(|d| d.id == id)
                .unwrap()
                .segments[index]
                .downloaded = received;
        }
        if failure.is_some() {
            break;
        }
        if received - committed >= 1024 * 1024 {
            checkpoint(inner, id, index, received, &mut file).await?;
            committed = received;
        }
    }
    checkpoint(inner, id, index, received, &mut file).await?;
    if let Some(error) = failure {
        return Err(error);
    }
    if segment
        .end
        .is_some_and(|end| segment.start + received != end)
    {
        return Err(Error::Length);
    }
    Ok(())
}

async fn checkpoint(
    inner: &Inner,
    id: DownloadId,
    index: usize,
    received: u64,
    file: &mut tokio::fs::File,
) -> Result<()> {
    #[cfg(debug_assertions)]
    if let Some(error) = crate::test_faults::checkpoint(id).await {
        return Err(error.into());
    }
    file.flush().await?;
    file.sync_data().await?;
    let mut data = inner.data.lock().await;
    data.downloads
        .iter_mut()
        .find(|d| d.id == id)
        .unwrap()
        .segments[index]
        .committed = received;
    inner.save(&data, id).await
}

async fn throttle(inner: &Inner, bytes: u64, token: &CancellationToken) -> Result<()> {
    loop {
        // Register before reading the rate so a settings change cannot be lost
        // between reserving bandwidth and beginning the wait.
        let changed = inner.rate_changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        let deadline = {
            let mut clock = inner.rate_clock.lock().await;
            let limit = inner.rate.load(Ordering::Relaxed);
            if limit == 0 {
                return if token.is_cancelled() {
                    Err(Error::Interrupted)
                } else {
                    Ok(())
                };
            }
            *clock = (*clock).max(tokio::time::Instant::now())
                + Duration::from_secs_f64(bytes as f64 / limit as f64);
            *clock
        };
        tokio::select! {
            _ = token.cancelled() => return Err(Error::Interrupted),
            _ = &mut changed => continue,
            _ = tokio::time::sleep_until(deadline) => return Ok(()),
        }
    }
}

async fn publish(inner: &Inner, id: DownloadId, token: &CancellationToken) -> Result<()> {
    #[cfg(debug_assertions)]
    if let Some(error) = crate::test_faults::publication_entry(id).await {
        return Err(error.into());
    }
    let mut data = inner.data.lock().await;
    if token.is_cancelled() {
        return Err(Error::Interrupted);
    }
    let download = data.downloads.iter_mut().find(|d| d.id == id).unwrap();
    if !download.ready_to_publish {
        let bytes = download.downloaded_bytes();
        if download.total_bytes.is_some_and(|n| n != bytes) {
            return Err(Error::Length);
        }
        download.total_bytes = Some(bytes);
        download.ready_to_publish = true;
        inner.save(&data, id).await?;
    }
    #[cfg(debug_assertions)]
    if let Some(error) = crate::test_faults::publication(id).await {
        return Err(error.into());
    }
    let download = data.downloads.iter_mut().find(|d| d.id == id).unwrap();
    // Linking publishes atomically without replacing a user's destination file. Both
    // paths share a directory. A retained link also makes crash recovery unambiguous.
    if let Err(error) = tokio::fs::hard_link(&download.temporary_path, &download.destination).await
        && (error.kind() != std::io::ErrorKind::AlreadyExists
            || !same_file::is_same_file(&download.temporary_path, &download.destination)
                .unwrap_or(false))
    {
        return Err(error.into());
    }
    download.state = DownloadState::Completed;
    let temporary_path = download.temporary_path.clone();
    if let Err(error) = inner.save(&data, id).await {
        data.downloads
            .iter_mut()
            .find(|d| d.id == id)
            .unwrap()
            .state = DownloadState::Downloading;
        return Err(error);
    }
    // Completion is already durable; a cleanup failure must not undo it.
    if let Err(error) = tokio::fs::remove_file(temporary_path).await {
        let _ = inner.events.send(EngineEvent::Error(format!(
            "completed download temporary-file cleanup failed: {error}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_content_disposition_filename;

    #[test]
    fn reads_quoted_and_unquoted_filenames() {
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=\"server-name.zip\""),
            Some("server-name.zip".into())
        );
        assert_eq!(
            parse_content_disposition_filename("inline; filename=server-name.zip"),
            Some("server-name.zip".into())
        );
    }

    #[test]
    fn prefers_rfc5987_extended_filenames() {
        assert_eq!(
            parse_content_disposition_filename(
                "attachment; filename=\"fallback.zip\"; filename*=UTF-8''caf%C3%A9%20menu.pdf"
            ),
            Some("café menu.pdf".into())
        );
    }

    #[test]
    fn handles_quoted_semicolons_and_escaped_quotes() {
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=\"report; final.zip\""),
            Some("report; final.zip".into())
        );
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=\"say \\\"hi\\\".zip\""),
            Some("say \"hi\".zip".into())
        );
    }

    #[test]
    fn strips_server_paths_and_rejects_empty_names() {
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=\"../../escape.exe\""),
            Some("escape.exe".into())
        );
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=\"..\""),
            None
        );
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=\"\""),
            None
        );
    }

    #[test]
    fn falls_back_when_extended_charset_is_not_utf8() {
        assert_eq!(
            parse_content_disposition_filename(
                "attachment; filename=\"fallback.zip\"; filename*=ISO-8859-1'en'%A3.zip"
            ),
            Some("fallback.zip".into())
        );
    }
}
