//! Interactive inspection shares transfer budgets but never creates a runnable job.
use crate::{Engine, Error, Result, engine::Inner, media_capture, transfer};
use futures_util::StreamExt;
use reqwest::{Url, header};
use std::{collections::HashSet, time::Duration};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

const MAX_REDIRECTS: usize = 10;
const MAX_ALLOWED_ORIGINS: usize = 32;
const INSPECTION_TIMEOUT: Duration = Duration::from_secs(60);

/// One bounded unauthenticated response. Redirect locations are untrusted and
/// must pass the caller's site policy before another request. No Debug because
/// Location can contain secret query strings.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SitePageResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub location: Option<String>,
    pub content_length: Option<u64>,
    pub body: Vec<u8>,
    /// Includes response bodies consumed by retries; charge this to site budgets.
    pub received_bytes: u64,
}

pub(crate) struct Inspector {
    pub(crate) client: reqwest::Client,
    pub shutdown: CancellationToken,
    pub operations: RwLock<()>,
}

impl Inspector {
    pub fn new() -> Result<Self> {
        Ok(Self {
            // Redirects are checked before sending, and each hop reacquires its
            // own host budget. Do not use the ordinary auto-redirecting client.
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(30))
                .user_agent("OSDM/0.1")
                .build()?,
            shutdown: CancellationToken::new(),
            operations: RwLock::new(()),
        })
    }
}

impl Engine {
    /// Fetch exactly one response, never following redirects. All network waits
    /// share ordinary engine admission/rate/cooldown budgets. The caller owns
    /// robots, origin and redirect policy; no credentials or referrer are sent.
    pub async fn fetch_site_page(
        &self,
        source: &str,
        max_bytes: usize,
        cancellation: &CancellationToken,
    ) -> Result<SitePageResponse> {
        self.fetch_site_response(source, max_bytes, cancellation, false)
            .await
    }

    /// One-hop HEAD metadata using the same admission/cancellation policy as GET.
    /// Content-Length describes the resource, not a downloaded response body.
    pub async fn head_site_page(
        &self,
        source: &str,
        cancellation: &CancellationToken,
    ) -> Result<SitePageResponse> {
        self.fetch_site_response(source, 0, cancellation, true)
            .await
    }

    async fn fetch_site_response(
        &self,
        source: &str,
        max_bytes: usize,
        cancellation: &CancellationToken,
        head: bool,
    ) -> Result<SitePageResponse> {
        if max_bytes > media_capture::MAX_MANIFEST_BYTES {
            return Err(media_capture::Error::Limit.into());
        }
        let inspector = &self.inner.media;
        let operation = async {
            let _active = inspector.operations.read().await;
            if self.inner.data.lock().await.stopped {
                return Err(Error::Shutdown);
            }
            let source = media_capture::MediaUrl::parse(source)?;
            let mut failures = 0;
            let mut received_bytes = 0;
            loop {
                let mut hint = crate::retry::RetryHint::default();
                let result = fetch_page_once(
                    &self.inner,
                    &source,
                    max_bytes,
                    cancellation,
                    &mut hint,
                    &mut received_bytes,
                    head,
                )
                .await;
                let retry = match &result {
                    Ok(page) => matches!(page.status, 429 | 500 | 502 | 503 | 504),
                    Err(error) => transfer::retryable(error),
                };
                if retry && failures < self.inner.config.max_retries {
                    transfer::wait_before_retry(
                        &self.inner,
                        source.as_str(),
                        failures,
                        hint,
                        cancellation,
                        None,
                    )
                    .await?;
                    failures += 1;
                    continue;
                }
                if hint.rate_limited {
                    let url =
                        Url::parse(source.as_str()).map_err(|_| media_capture::Error::UnsafeUrl)?;
                    self.inner
                        .cooldowns
                        .defer(
                            url.host_str().ok_or(media_capture::Error::UnsafeUrl)?,
                            crate::retry::delay(failures, hint.after),
                        )
                        .await;
                }
                return result;
            }
        };
        tokio::select! {
            biased;
            _ = inspector.shutdown.cancelled() => Err(Error::Shutdown),
            _ = cancellation.cancelled() => Err(Error::Interrupted),
            result = tokio::time::timeout(INSPECTION_TIMEOUT, operation) =>
                result.unwrap_or_else(|_| Err(Error::Invalid("site fetch timed out".into()))),
        }
    }

    /// Fetch and inspect one bounded HLS manifest, without enqueueing media.
    ///
    /// Only the supplied source origin is authorized by default. Additional
    /// redirect origins must be canonical HTTP(S) origins (no path/query) the
    /// user explicitly approved. Manifest references are returned, never fetched;
    /// a CDN reference is not network or credential authorization. No cookies,
    /// captured headers, Basic credentials, or referrer are sent by this method.
    /// Cancellation, shutdown, or the 60-second overall deadline drops all reads
    /// and releases all budgets. Source URLs must never be logged by callers.
    pub async fn inspect_hls(
        &self,
        source: &str,
        allowed_redirect_origins: &[String],
        cancellation: &CancellationToken,
    ) -> Result<media_capture::Manifest> {
        let inspector = &self.inner.media;
        let token = CancellationToken::new();
        let operation = async {
            let _active = inspector.operations.read().await;
            if self.inner.data.lock().await.stopped {
                return Err(Error::Shutdown);
            }
            // Validate everything before the first network request.
            let source = media_capture::MediaUrl::parse(source)?;
            let mut origins = HashSet::from([origin(source.as_str())?]);
            if allowed_redirect_origins.len() > MAX_ALLOWED_ORIGINS {
                return Err(media_capture::Error::Limit.into());
            }
            for allowed in allowed_redirect_origins {
                let parsed = media_capture::MediaUrl::parse(allowed)?;
                let canonical = origin(parsed.as_str())?;
                if allowed != &canonical {
                    return Err(Error::Invalid("expected a canonical HTTP(S) origin".into()));
                }
                origins.insert(canonical);
            }
            fetch(&self.inner, source, &origins, &token).await
        };
        tokio::select! {
            biased;
            _ = inspector.shutdown.cancelled() => Err(Error::Shutdown),
            _ = cancellation.cancelled() => Err(Error::Interrupted),
            result = tokio::time::timeout(INSPECTION_TIMEOUT, operation) =>
                result.unwrap_or_else(|_| Err(Error::Invalid("media inspection timed out".into()))),
        }
    }
}

async fn fetch_page_once(
    inner: &Inner,
    source: &media_capture::MediaUrl,
    max_bytes: usize,
    token: &CancellationToken,
    hint: &mut crate::retry::RetryHint,
    received_bytes: &mut u64,
    head: bool,
) -> Result<SitePageResponse> {
    let _permits = transfer::permits(inner, source.as_str(), token, None, false).await?;
    let response = inner
        .media
        .client
        .request(
            if head {
                reqwest::Method::HEAD
            } else {
                reqwest::Method::GET
            },
            source.as_str(),
        )
        .header(header::ACCEPT_ENCODING, "identity")
        .send()
        .await?;
    let status = response.status().as_u16();
    if matches!(status, 429 | 503) {
        *hint = transfer::retry_hint(&response);
    }
    if response
        .headers()
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .any(|v| v != "identity")
    {
        return Err(Error::Invalid(
            "encoded site responses are unsupported".into(),
        ));
    }
    let text_header = |name| -> Result<Option<String>> {
        response
            .headers()
            .get(name)
            .map(|value| {
                let value = value
                    .to_str()
                    .map_err(|_| Error::Invalid("invalid site response header".into()))?;
                if value.len() > 8192 || value.chars().any(char::is_control) {
                    return Err(media_capture::Error::Limit.into());
                }
                Ok(value.to_owned())
            })
            .transpose()
    };
    let content_type = text_header(header::CONTENT_TYPE)?;
    let location = text_header(header::LOCATION)?;
    let content_length = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| Error::Invalid("invalid site Content-Length".into()))
        })
        .transpose()?;
    if !head && content_length.is_some_and(|length| length > max_bytes as u64 - *received_bytes) {
        return Err(media_capture::Error::Limit.into());
    }
    let mut body = Vec::new();
    if head {
        return Ok(SitePageResponse {
            status,
            content_type,
            location,
            content_length,
            body,
            received_bytes: *received_bytes,
        });
    }
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if chunk.len() as u64 > max_bytes as u64 - *received_bytes {
            return Err(media_capture::Error::Limit.into());
        }
        transfer::throttle(inner, chunk.len() as u64, token).await?;
        *received_bytes += chunk.len() as u64;
        body.extend_from_slice(&chunk);
    }
    Ok(SitePageResponse {
        status,
        content_type,
        location,
        content_length,
        body,
        received_bytes: *received_bytes,
    })
}

fn origin(url: &str) -> Result<String> {
    Ok(Url::parse(url)
        .map_err(|_| media_capture::Error::UnsafeUrl)?
        .origin()
        .ascii_serialization())
}

async fn fetch(
    inner: &Inner,
    source: media_capture::MediaUrl,
    origins: &HashSet<String>,
    token: &CancellationToken,
) -> Result<media_capture::Manifest> {
    let mut current = source;
    let mut seen = HashSet::new();
    let mut redirects = 0;
    loop {
        if !seen.insert(current.clone()) {
            return Err(Error::Invalid("media redirect loop".into()));
        }
        let (body, next) = fetch_with_retries(inner, &current, token).await?;
        if let Some(next) = next {
            redirects += 1;
            if redirects > MAX_REDIRECTS {
                return Err(Error::Invalid("too many media redirects".into()));
            }
            authorize_redirect(&current, &next, origins)?;
            current = next;
        } else {
            return Ok(media_capture::inspect_hls(current.as_str(), &body)?);
        }
    }
}

fn authorize_redirect(
    current: &media_capture::MediaUrl,
    next: &media_capture::MediaUrl,
    origins: &HashSet<String>,
) -> Result<()> {
    if (current.as_str().starts_with("https:") && !next.as_str().starts_with("https:"))
        || !origins.contains(&origin(next.as_str())?)
    {
        return Err(Error::Invalid(
            "media redirect origin is not authorized".into(),
        ));
    }
    Ok(())
}

async fn fetch_with_retries(
    inner: &Inner,
    source: &media_capture::MediaUrl,
    token: &CancellationToken,
) -> Result<(Vec<u8>, Option<media_capture::MediaUrl>)> {
    let mut failures = 0;
    loop {
        let mut hint = crate::retry::RetryHint::default();
        match fetch_once(inner, source, token, &mut hint).await {
            Ok(result) => return Ok(result),
            Err(error) if transfer::retryable(&error) && failures < inner.config.max_retries => {
                transfer::wait_before_retry(inner, source.as_str(), failures, hint, token, None)
                    .await?;
                failures += 1;
            }
            Err(error) => {
                // Even an exhausted interactive request updates shared cooldowns.
                if hint.rate_limited {
                    let host =
                        Url::parse(source.as_str()).map_err(|_| media_capture::Error::UnsafeUrl)?;
                    inner
                        .cooldowns
                        .defer(
                            host.host_str().ok_or(media_capture::Error::UnsafeUrl)?,
                            crate::retry::delay(failures, hint.after),
                        )
                        .await;
                }
                return Err(error);
            }
        }
    }
}

async fn fetch_once(
    inner: &Inner,
    source: &media_capture::MediaUrl,
    token: &CancellationToken,
    hint: &mut crate::retry::RetryHint,
) -> Result<(Vec<u8>, Option<media_capture::MediaUrl>)> {
    let _permits = transfer::permits(inner, source.as_str(), token, None, false).await?;
    let response = inner
        .media
        .client
        .get(source.as_str())
        .header(header::ACCEPT_ENCODING, "identity")
        .header(
            header::ACCEPT,
            "application/vnd.apple.mpegurl, application/x-mpegURL, text/plain",
        )
        .send()
        .await?;
    let status = response.status().as_u16();
    if matches!(status, 301 | 302 | 303 | 307 | 308) {
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::Invalid("invalid media redirect".into()))?;
        if location.len() > 8192
            || location.trim() != location
            || location.chars().any(char::is_control)
        {
            return Err(media_capture::Error::UnsafeUrl.into());
        }
        let next = response
            .url()
            .join(location)
            .map_err(|_| media_capture::Error::UnsafeUrl)?;
        return Ok((
            Vec::new(),
            Some(media_capture::MediaUrl::parse(next.as_str())?),
        ));
    }
    if matches!(status, 429 | 503) {
        *hint = transfer::retry_hint(&response);
    }
    if status != 200 {
        return Err(Error::Http(status));
    }
    if response
        .headers()
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .any(|value| value != "identity")
    {
        return Err(Error::Invalid(
            "encoded media manifests are unsupported".into(),
        ));
    }
    if response
        .content_length()
        .is_some_and(|len| len > media_capture::MAX_MANIFEST_BYTES as u64)
    {
        return Err(media_capture::Error::Limit.into());
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if chunk.len() > media_capture::MAX_MANIFEST_BYTES - body.len() {
            return Err(media_capture::Error::Limit.into());
        }
        transfer::throttle(inner, chunk.len() as u64, token).await?;
        body.extend_from_slice(&chunk);
    }
    Ok((body, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_allowlist_never_authorizes_tls_downgrade() {
        let secure = media_capture::MediaUrl::parse("https://media.example.test/index").unwrap();
        let insecure = media_capture::MediaUrl::parse("http://media.example.test/index").unwrap();
        let origins = HashSet::from([
            "https://media.example.test".into(),
            "http://media.example.test".into(),
        ]);
        assert!(authorize_redirect(&secure, &insecure, &origins).is_err());
        assert!(authorize_redirect(&insecure, &secure, &origins).is_ok());
        assert!(authorize_redirect(&secure, &secure, &origins).is_ok());
        assert!(authorize_redirect(&secure, &secure, &HashSet::new()).is_err());
    }

    #[tokio::test]
    async fn overall_deadline_and_cancellation_cover_engine_admission() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path().join("inspection.db"), crate::Config::default())
            .await
            .unwrap();
        let data = engine.inner.data.lock().await;
        tokio::time::pause();
        let token = CancellationToken::new();
        let operation = engine.inspect_hls("http://127.0.0.1:1/index", &[], &token);
        tokio::pin!(operation);
        tokio::select! {
            biased;
            _ = &mut operation => panic!("admission must wait for the engine lock"),
            _ = tokio::task::yield_now() => {}
        }
        tokio::time::advance(INSPECTION_TIMEOUT + Duration::from_secs(1)).await;
        assert!(
            matches!(operation.await, Err(Error::Invalid(message)) if message == "media inspection timed out")
        );
        tokio::time::resume();

        let operation = engine.inspect_hls("http://127.0.0.1:1/index", &[], &token);
        tokio::pin!(operation);
        tokio::select! {
            biased;
            _ = &mut operation => panic!("admission must wait for the engine lock"),
            _ = tokio::task::yield_now() => {}
        }
        token.cancel();
        assert!(matches!(operation.await, Err(Error::Interrupted)));
        drop(data);
        engine.shutdown().await.unwrap();
    }
}
