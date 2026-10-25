//! Retry scheduling shared by probes and segment workers.
//!
//! Rate limiting is handled in three layers:
//!
//! - a server-supplied `Retry-After` header (delta-seconds or HTTP-date) is
//!   honored, including on probe requests;
//! - otherwise retries use bounded exponential backoff with jitter so parallel
//!   workers do not retry in lockstep;
//! - a rate-limited host is put on a shared cooldown, so other workers and other
//!   downloads stop sending requests to it until the window expires.

use crate::{Error, Result};
use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// First fallback delay; doubles with every failed attempt.
const BASE_DELAY: Duration = Duration::from_secs(1);
/// Upper bound for fallback backoff. Server-requested waits are not shortened.
const MAX_DELAY: Duration = Duration::from_secs(120);

/// What the last failed attempt learned about when it is safe to retry.
#[derive(Clone, Copy, Default)]
pub(crate) struct RetryHint {
    /// Delay requested by the server's `Retry-After` header, when readable.
    pub(crate) after: Option<Duration>,
    /// The server rejected the request as rate limited (429 or 503).
    pub(crate) rate_limited: bool,
}

/// Parse a `Retry-After` header value as delta-seconds or an HTTP-date.
/// Returns `None` for values that cannot be interpreted.
pub(crate) fn parse_retry_after(value: &str) -> Option<Duration> {
    parse_retry_after_at(value, SystemTime::now())
}

fn parse_retry_after_at(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.parse().ok().map(Duration::from_secs);
    }
    let deadline = httpdate::parse_http_date(value).ok()?;
    // A date in the past means "retry now"; the exponential floor below still
    // prevents an immediate retry storm.
    Some(deadline.duration_since(now).unwrap_or(Duration::ZERO))
}

/// Delay before retry number `attempt` (zero-based). Never retry before the
/// server's requested delay or the jittered exponential fallback.
pub(crate) fn delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
    let exponential = BASE_DELAY
        .saturating_mul(1u32 << attempt.min(7))
        .min(MAX_DELAY);
    let fallback = spread(exponential).min(MAX_DELAY);
    retry_after.map_or(fallback, |value| value.max(fallback))
}

/// Add up to ten percent jitter so segmented workers and simultaneous
/// downloads do not retry at the same instant.
fn spread(base: Duration) -> Duration {
    let millis = u64::try_from(base.as_millis()).unwrap_or(u64::MAX);
    let extra = (millis / 10).max(1);
    base.saturating_add(Duration::from_millis(random() % extra))
}

fn random() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}

/// Cancellable wait. Every retry delay must stop immediately on pause/cancel.
pub(crate) async fn sleep(mut delay: Duration, token: &CancellationToken) -> Result<()> {
    // Chunk untrusted Retry-After durations to avoid overflowing timer deadlines.
    loop {
        let chunk = delay.min(Duration::from_secs(24 * 3600));
        tokio::select! {
            _ = token.cancelled() => return Err(Error::Interrupted),
            _ = tokio::time::sleep(chunk) => {}
        }
        delay -= chunk;
        if delay.is_zero() {
            return Ok(());
        }
    }
}

/// Per-host rate-limit windows. A host is blocked until the latest deadline
/// reported by any worker; later, shorter windows never shorten an earlier one.
#[derive(Default)]
pub(crate) struct Cooldowns(Mutex<HashMap<String, tokio::time::Instant>>);

impl Cooldowns {
    /// Extend `host`'s cooldown to at least `delay` from now.
    pub(crate) async fn defer(&self, host: &str, delay: Duration) {
        let now = tokio::time::Instant::now();
        // Untrusted headers can contain durations larger than Instant supports.
        let deadline = now
            .checked_add(delay)
            .unwrap_or_else(|| now + Duration::from_secs(100 * 365 * 24 * 3600));
        let mut hosts = self.0.lock().await;
        hosts.retain(|_, until| *until > now);
        hosts
            .entry(host.to_owned())
            .and_modify(|until| *until = (*until).max(deadline))
            .or_insert(deadline);
    }

    /// Wait for `host`'s cooldown to expire, re-checking whenever another
    /// worker extends it.
    pub(crate) async fn wait(&self, host: &str, token: &CancellationToken) -> Result<()> {
        loop {
            let deadline = {
                let mut hosts = self.0.lock().await;
                match hosts.get(host).copied() {
                    Some(deadline) if deadline > tokio::time::Instant::now() => deadline,
                    Some(_) => {
                        hosts.remove(host);
                        return Ok(());
                    }
                    None => return Ok(()),
                }
            };
            tokio::select! {
                _ = token.cancelled() => return Err(Error::Interrupted),
                _ = tokio::time::sleep(deadline.saturating_duration_since(tokio::time::Instant::now()).min(Duration::from_secs(24 * 3600))) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(value: &str) -> SystemTime {
        httpdate::parse_http_date(value).unwrap()
    }

    #[test]
    fn reads_delta_seconds() {
        assert_eq!(parse_retry_after("0"), Some(Duration::ZERO));
        assert_eq!(parse_retry_after("90"), Some(Duration::from_secs(90)));
        assert_eq!(parse_retry_after("  7  "), Some(Duration::from_secs(7)));
    }

    #[test]
    fn reads_http_dates() {
        let now = time("Wed, 21 Oct 2015 07:27:00 GMT");
        assert_eq!(
            parse_retry_after_at("Wed, 21 Oct 2015 07:28:00 GMT", now),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            parse_retry_after_at("Wed, 21 Oct 2015 07:27:30 GMT", now),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            parse_retry_after_at("Wed, 21 Oct 2015 07:26:00 GMT", now),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn rejects_unreadable_values() {
        assert_eq!(parse_retry_after(""), None);
        assert_eq!(parse_retry_after("soon"), None);
        assert_eq!(parse_retry_after("-5"), None);
        assert_eq!(parse_retry_after("18446744073709551616"), None);
    }

    #[test]
    fn falls_back_to_exponential_backoff() {
        let expected = [1, 2, 4, 8, 16];
        for (attempt, seconds) in expected.iter().enumerate() {
            let delay = delay(attempt as u32, None);
            assert!(delay >= Duration::from_secs(*seconds));
            assert!(delay <= Duration::from_secs(*seconds) * 11 / 10);
        }
    }

    #[test]
    fn explicit_retry_after_wins_but_zero_is_ignored() {
        let delay = super::delay(0, Some(Duration::from_secs(30)));
        assert!(delay >= Duration::from_secs(30));
        assert!(delay < Duration::from_secs(34));

        // `Retry-After: 0` must not turn into a request storm.
        let delay = super::delay(2, Some(Duration::ZERO));
        assert!(delay >= Duration::from_secs(4));
    }

    #[test]
    fn caps_fallback_but_never_shortens_explicit_retry_after() {
        assert!(super::delay(10, Some(Duration::from_secs(3600))) >= Duration::from_secs(3600));
        assert!(super::delay(9, None) <= Duration::from_secs(120));
    }

    #[tokio::test(start_paused = true)]
    async fn hosts_wait_their_own_cooldown() {
        let cooldowns = Cooldowns::default();
        let token = CancellationToken::new();
        let start = tokio::time::Instant::now();
        cooldowns
            .defer("slow.example", Duration::from_secs(5))
            .await;

        // An unrelated host is not delayed.
        cooldowns.wait("other.example", &token).await.unwrap();
        assert!(start.elapsed() < Duration::from_secs(1));

        cooldowns.wait("slow.example", &token).await.unwrap();
        assert!(start.elapsed() >= Duration::from_secs(5));
    }

    #[tokio::test(start_paused = true)]
    async fn cooldown_can_be_extended_and_cancelled() {
        let cooldowns = Cooldowns::default();
        let token = CancellationToken::new();
        cooldowns
            .defer("slow.example", Duration::from_secs(3))
            .await;
        cooldowns
            .defer("slow.example", Duration::from_secs(6))
            .await;

        let cancelled = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            cancelled.cancel();
        });
        assert!(matches!(
            cooldowns.wait("slow.example", &token).await,
            Err(Error::Interrupted)
        ));
    }
}
