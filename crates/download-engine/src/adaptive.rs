//! Per-host feedback controller. Limits are ceilings, not a fixed worker count.
//! Leases retire excess ranged requests cooperatively, after their checkpoint.
use crate::{AdaptiveStatus, DownloadId, Error, Result};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::Notify, time::Instant};
use tokio_util::sync::CancellationToken;

const WINDOW: Duration = Duration::from_secs(2);
const MIN_CHUNK: u64 = 256 * 1024;
const MAX_CHUNK: u64 = 8 * 1024 * 1024;

struct Active {
    download: Option<DownloadId>,
    retire: CancellationToken,
    yieldable: bool,
    started: bool,
}

struct Trial {
    previous_limit: usize,
    baseline: f64,
    samples: u32,
    total_rate: f64,
}

struct State {
    ceiling: usize,
    limit: usize,
    next: u64,
    active: BTreeMap<u64, Active>,
    waiting: usize,
    window: Instant,
    bytes: u64,
    rate: f64,
    latency: Option<Duration>,
    healthy: u32,
    slow: u32,
    hold_until: Instant,
    last_reduction: Option<Instant>,
    trial: Option<Trial>,
    chunk: u64,
    reason: &'static str,
}

pub(crate) struct Host {
    state: Mutex<State>,
    changed: Notify,
}

pub(crate) struct Lease {
    host: Arc<Host>,
    key: u64,
    pub(crate) retire: CancellationToken,
}

struct Waiter(Arc<Host>);
impl Drop for Waiter {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().waiting -= 1;
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.host.state.lock().unwrap().active.remove(&self.key);
        self.host.changed.notify_waiters();
    }
}

impl Lease {
    pub(crate) fn started(&self) {
        if let Some(active) = self.host.state.lock().unwrap().active.get_mut(&self.key) {
            active.started = true;
        }
    }
    pub(crate) fn received(&self, bytes: u64) {
        let mut state = self.host.state.lock().unwrap();
        state.bytes = state.bytes.saturating_add(bytes);
    }

    pub(crate) fn response(&self, latency: Duration) {
        let mut state = self.host.state.lock().unwrap();
        if state
            .latency
            .is_some_and(|baseline| latency > (baseline * 3).max(Duration::from_secs(2)))
        {
            reduce(&mut state, "Server response slowed; reducing connections");
        }
        state.latency = Some(state.latency.map_or(latency, |old| (old * 3 + latency) / 4));
        drop(state);
        self.host.changed.notify_waiters();
    }
}

impl Host {
    pub(crate) fn new(ceiling: usize) -> Self {
        let now = Instant::now();
        Self {
            state: Mutex::new(State {
                ceiling,
                limit: ceiling.clamp(1, 2),
                next: 0,
                active: BTreeMap::new(),
                waiting: 0,
                window: now,
                bytes: 0,
                rate: 0.0,
                latency: None,
                healthy: 0,
                slow: 0,
                hold_until: now + WINDOW * 3,
                last_reduction: None,
                trial: None,
                chunk: 1024 * 1024,
                reason: "Starting cautiously; measuring throughput",
            }),
            changed: Notify::new(),
        }
    }

    pub(crate) async fn acquire(
        self: &Arc<Self>,
        download: Option<DownloadId>,
        yieldable: bool,
        token: &CancellationToken,
    ) -> Result<Lease> {
        self.state.lock().unwrap().waiting += 1;
        let _waiter = Waiter(self.clone());
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if token.is_cancelled() {
                return Err(Error::Interrupted);
            }
            {
                let mut state = self.state.lock().unwrap();
                if state.active.len() < state.limit {
                    let key = state.next;
                    state.next = state.next.wrapping_add(1);
                    let retire = CancellationToken::new();
                    state.active.insert(
                        key,
                        Active {
                            download,
                            retire: retire.clone(),
                            yieldable,
                            started: false,
                        },
                    );
                    return Ok(Lease {
                        host: self.clone(),
                        key,
                        retire,
                    });
                }
            }
            tokio::select! {
                _ = token.cancelled() => return Err(Error::Interrupted),
                _ = &mut changed => {}
            }
        }
    }

    pub(crate) fn congested(&self, rate_limited: bool) {
        let mut state = self.state.lock().unwrap();
        reduce(
            &mut state,
            if rate_limited {
                "Server rate limit; fewer connections and smaller requests"
            } else {
                "Network errors; fewer connections and smaller requests"
            },
        );
        drop(state);
        self.changed.notify_waiters();
    }

    pub(crate) fn chunk_size(&self) -> u64 {
        self.state.lock().unwrap().chunk
    }

    pub(crate) fn retry_floor(&self) -> Duration {
        let state = self.state.lock().unwrap();
        if Instant::now() < state.hold_until && state.last_reduction.is_some() {
            Duration::from_secs(4)
        } else {
            Duration::ZERO
        }
    }

    /// Called by the engine's existing heartbeat; no extra timer/task per host.
    pub(crate) fn tick(&self, bandwidth_limited: bool) {
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        let elapsed = now.duration_since(state.window);
        if elapsed < WINDOW {
            return;
        }
        let rate = state.bytes as f64 / elapsed.as_secs_f64();
        state.bytes = 0;
        state.window = now;
        let active = state.active.values().filter(|a| a.started).count();
        if active == 0 || bandwidth_limited {
            // User throttling and idle periods are not evidence of a bad network.
            state.healthy = 0;
            state.slow = 0;
            state.trial = None;
            state.rate = 0.0;
            if bandwidth_limited {
                state.reason = "User speed limit active; holding connection count";
            }
            return;
        }
        if state.reason == "User speed limit active; holding connection count" {
            state.reason = "Speed limit removed; measuring throughput";
        }
        if rate == 0.0 || (state.rate > 0.0 && rate < state.rate * 0.6) {
            state.slow += 1;
        } else {
            state.slow = 0;
        }
        if state.slow >= 2 {
            reduce(
                &mut state,
                "Throughput degraded or stalled; reducing connections",
            );
            state.rate = rate;
            drop(state);
            self.changed.notify_waiters();
            return;
        }
        if let Some(mut trial) = state.trial.take() {
            trial.samples += 1;
            trial.total_rate += rate;
            if trial.samples >= 2 {
                if (trial.total_rate / trial.samples as f64) < trial.baseline * 1.08 {
                    state.limit = trial.previous_limit;
                    retire_excess(&state);
                    state.reason = "Extra connection did not improve throughput; scaling back";
                } else {
                    state.reason = "Additional connection improved throughput";
                }
                state.hold_until = now + Duration::from_secs(20);
                state.healthy = 0;
            } else {
                state.trial = Some(trial);
            }
        } else if rate > 0.0 && state.slow == 0 {
            state.healthy += 1;
            if state.healthy >= 3
                && now >= state.hold_until
                && state.waiting > 0
                && state.limit < state.ceiling
            {
                state.trial = Some(Trial {
                    previous_limit: state.limit,
                    baseline: state.rate.max(rate),
                    samples: 0,
                    total_rate: 0.0,
                });
                state.limit += 1;
                state.healthy = 0;
                state.reason = "Healthy network; testing one additional connection";
            }
        }
        state.rate = if state.rate == 0.0 {
            rate
        } else {
            state.rate * 0.7 + rate * 0.3
        };
        if now >= state.hold_until && rate > 0.0 {
            state.chunk =
                ((state.rate / active.max(1) as f64 * 2.0) as u64).clamp(MIN_CHUNK, MAX_CHUNK);
        }
        drop(state);
        self.changed.notify_waiters();
    }

    pub(crate) fn snapshot(&self, id: DownloadId) -> AdaptiveStatus {
        let state = self.state.lock().unwrap();
        AdaptiveStatus {
            active_connections: state
                .active
                .values()
                .filter(|a| a.started && a.download == Some(id))
                .count(),
            host_connections: state.active.values().filter(|a| a.started).count(),
            host_limit: state.limit,
            host_ceiling: state.ceiling,
            chunk_bytes: state.chunk,
            reason: state.reason.into(),
        }
    }
}

fn reduce(state: &mut State, reason: &'static str) {
    let now = Instant::now();
    // One burst of failures is one congestion signal, not N halvings.
    if state
        .last_reduction
        .is_some_and(|last| now.duration_since(last) < WINDOW)
    {
        return;
    }
    state.limit = (state.limit / 2).max(1);
    state.chunk = (state.chunk / 2).max(MIN_CHUNK);
    state.last_reduction = Some(now);
    state.hold_until = now + Duration::from_secs(20);
    state.healthy = 0;
    state.slow = 0;
    state.trial = None;
    state.reason = reason;
    retire_excess(state);
}

fn retire_excess(state: &State) {
    let excess = state.active.len().saturating_sub(state.limit);
    for active in state
        .active
        .values()
        .rev()
        .filter(|a| a.yieldable)
        .take(excess)
    {
        active.retire.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> DownloadId {
        DownloadId::new_v4()
    }

    #[tokio::test]
    async fn rate_limit_reduces_connections_and_coalesces_a_burst() {
        let host = Arc::new(Host::new(8));
        let download = id();
        let first = host
            .acquire(Some(download), true, &CancellationToken::new())
            .await
            .unwrap();
        let second = host
            .acquire(Some(download), true, &CancellationToken::new())
            .await
            .unwrap();
        first.started();
        second.started();
        let initial_chunk = host.chunk_size();

        host.congested(true);
        assert!(first.retire.is_cancelled() || second.retire.is_cancelled());
        let status = host.snapshot(download);
        assert_eq!(status.host_limit, 1);
        assert_eq!(host.chunk_size(), initial_chunk / 2);

        host.congested(true);
        assert_eq!(
            host.snapshot(download).host_limit,
            1,
            "burst should be one signal"
        );
        assert!(host.retry_floor() >= Duration::from_secs(4));
    }

    #[tokio::test]
    async fn latency_degradation_reduces_connection_limit() {
        let host = Arc::new(Host::new(8));
        let lease = host
            .acquire(Some(id()), true, &CancellationToken::new())
            .await
            .unwrap();
        lease.started();
        lease.response(Duration::from_millis(20));
        lease.response(Duration::from_secs(3));
        assert_eq!(host.snapshot(id()).host_limit, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn degraded_throughput_reduces_connection_limit_after_confirmed_windows() {
        let host = Arc::new(Host::new(8));
        let download = id();
        let lease = host
            .acquire(Some(download), true, &CancellationToken::new())
            .await
            .unwrap();
        lease.started();
        host.state.lock().unwrap().rate = 1_000_000.0;

        tokio::time::advance(WINDOW).await;
        host.tick(false);
        assert_eq!(
            host.snapshot(download).host_limit,
            2,
            "one slow window is not enough"
        );
        tokio::time::advance(WINDOW).await;
        host.tick(false);
        assert_eq!(host.snapshot(download).host_limit, 1);
        assert!(
            host.snapshot(download)
                .reason
                .contains("Throughput degraded")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn good_throughput_trials_one_connection_and_rolls_back_no_gain() {
        let host = Arc::new(Host::new(4));
        let download = id();
        let first = host
            .acquire(Some(download), true, &CancellationToken::new())
            .await
            .unwrap();
        let second = host
            .acquire(Some(download), true, &CancellationToken::new())
            .await
            .unwrap();
        first.started();
        second.started();
        host.state.lock().unwrap().hold_until = Instant::now();
        let waiting_host = host.clone();
        let waiter = tokio::spawn(async move {
            waiting_host
                .acquire(Some(download), true, &CancellationToken::new())
                .await
        });
        while host.state.lock().unwrap().waiting == 0 {
            tokio::task::yield_now().await;
        }

        for _ in 0..3 {
            first.received(2_000_000);
            second.received(2_000_000);
            tokio::time::advance(WINDOW).await;
            host.tick(false);
        }
        assert_eq!(host.snapshot(download).host_limit, 3);
        assert!(host.state.lock().unwrap().trial.is_some());
        // Two trial windows at the prior throughput make the extra lease useless.
        for _ in 0..2 {
            first.received(2_000_000);
            second.received(2_000_000);
            tokio::time::advance(WINDOW).await;
            host.tick(false);
        }
        assert_eq!(host.snapshot(download).host_limit, 2);
        assert_eq!(
            host.snapshot(download).reason,
            "Extra connection did not improve throughput; scaling back"
        );

        waiter.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn ceiling_is_honored_and_speed_limit_holds_tuning() {
        let host = Arc::new(Host::new(1));
        let download = id();
        let lease = host
            .acquire(Some(download), true, &CancellationToken::new())
            .await
            .unwrap();
        lease.started();
        host.state.lock().unwrap().hold_until = Instant::now();
        for _ in 0..8 {
            lease.received(10_000_000);
            tokio::time::advance(WINDOW).await;
            host.tick(true);
        }
        let status = host.snapshot(download);
        assert_eq!(status.host_limit, 1);
        assert_eq!(status.host_ceiling, 1);
        assert!(status.reason.contains("User speed limit"));
        lease.received(10_000_000);
        tokio::time::advance(WINDOW).await;
        host.tick(false);
        assert_eq!(
            host.snapshot(download).reason,
            "Speed limit removed; measuring throughput"
        );
    }

    #[tokio::test]
    async fn congestion_retires_excess_and_cancellation_releases_blocked_admission() {
        let host = Arc::new(Host::new(4));
        let first = host
            .acquire(None, true, &CancellationToken::new())
            .await
            .unwrap();
        let second = host
            .acquire(None, true, &CancellationToken::new())
            .await
            .unwrap();
        first.started();
        second.started();
        host.congested(false);
        assert!(first.retire.is_cancelled() || second.retire.is_cancelled());

        let token = CancellationToken::new();
        let waiting_host = host.clone();
        let waiting_token = token.clone();
        let waiter =
            tokio::spawn(async move { waiting_host.acquire(None, true, &waiting_token).await });
        while host.state.lock().unwrap().waiting == 0 {
            tokio::task::yield_now().await;
        }
        token.cancel();
        assert!(matches!(waiter.await.unwrap(), Err(Error::Interrupted)));
        assert_eq!(host.state.lock().unwrap().waiting, 0);
    }
}
