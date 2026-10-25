use crate::{DownloadId, DownloadState, Engine, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// UTC seconds, never local wall-clock arithmetic. Recurrences remain anchored
/// to their original instant across DST, timezone changes, sleep and clock jumps.
pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn queue_has_workers(data: &crate::engine::Data, id: DownloadId) -> bool {
    data.synchronizing.contains_key(&id)
        || data.downloads.iter().any(|d| {
            data.automation.membership.get(&d.id) == Some(&id) && data.running.contains_key(&d.id)
        })
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Automation {
    pub queues: Vec<DownloadQueue>,
    pub schedules: Vec<QueueSchedule>,
    pub membership: BTreeMap<DownloadId, DownloadId>,
    #[serde(default)]
    pub pending_captures: BTreeMap<DownloadId, u64>,
    #[serde(default)]
    pub sync_roots: BTreeMap<DownloadId, DownloadId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadQueue {
    pub id: DownloadId,
    pub name: String,
    pub running: bool,
    pub stop_at: Option<u64>,
    #[serde(default)]
    pub synchronize: bool,
    #[serde(default)]
    pub checking_sources: bool,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueSchedule {
    pub id: DownloadId,
    pub queue_id: DownloadId,
    /// Next unconsumed slot; saved transactionally before dispatch.
    pub next_run: u64,
    pub interval_seconds: Option<u64>,
    /// Stop this run after a bounded window; None means until completion.
    pub stop_after_seconds: Option<u64>,
    pub run_once_on_startup: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationCommand {
    SetSynchronization {
        id: DownloadId,
        enabled: bool,
    },
    SaveQueue {
        id: DownloadId,
        name: String,
    },
    DeleteQueue {
        id: DownloadId,
    },
    Assign {
        ids: Vec<DownloadId>,
        queue_id: Option<DownloadId>,
    },
    SaveSchedule {
        schedule: QueueSchedule,
    },
    DeleteSchedule {
        id: DownloadId,
    },
    Start {
        id: DownloadId,
    },
    Stop {
        id: DownloadId,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchItem {
    pub request: crate::DownloadRequest,
    pub queue_id: Option<DownloadId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchResult {
    pub index: usize,
    pub id: Option<DownloadId>,
    pub duplicate: bool,
    /// Stable, deliberately URL/path-free code. Admission is per item, not atomic.
    pub error: Option<String>,
}

impl Automation {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.queues.len() > 128 || self.schedules.len() > 512 {
            return Err(Error::Invalid("too many queues or schedules".into()));
        }
        let mut ids = HashSet::new();
        for q in &self.queues {
            if !ids.insert(q.id)
                || q.name.trim().is_empty()
                || q.name.len() > 128
                || q.name.chars().any(char::is_control)
            {
                return Err(Error::Invalid("invalid queue".into()));
            }
        }
        let mut schedules = HashSet::new();
        for s in &self.schedules {
            if !schedules.insert(s.id)
                || !ids.contains(&s.queue_id)
                || s.next_run > 253402300799
                || s.interval_seconds
                    .is_some_and(|n| !(60..=31_536_000).contains(&n))
                || s.stop_after_seconds
                    .is_some_and(|n| !(1..=31_536_000).contains(&n))
            {
                return Err(Error::Invalid("invalid schedule".into()));
            }
        }
        if self.membership.values().any(|id| !ids.contains(id)) {
            return Err(Error::Invalid("unknown queue assignment".into()));
        }
        Ok(())
    }
}

impl Engine {
    /// Commit/abort are idempotent. Only a prepared capture can be aborted;
    /// ordinary history or an already committed transfer is never removed.
    pub async fn commit_capture(&self, id: DownloadId) -> Result<()> {
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        let Some(expiry) = data.automation.pending_captures.get(&id) else {
            return if data.downloads.iter().any(|d| d.id == id) {
                Ok(())
            } else {
                Err(Error::NotFound)
            };
        };
        if *expiry <= unix_now() {
            return Err(Error::Invalid("capture lease expired".into()));
        }
        let mut next = data.automation.clone();
        next.pending_captures.remove(&id);
        let mut downloads = data.downloads.clone();
        let download = downloads
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(Error::NotFound)?;
        if download.state != DownloadState::Paused {
            return Err(Error::Invalid("capture was stopped by the user".into()));
        }
        download.state = DownloadState::Queued;
        self.inner.store.save_automation(&next, &downloads).await?;
        data.automation = next;
        data.downloads = downloads;
        Ok(())
    }

    pub async fn abort_capture(&self, id: DownloadId) -> Result<()> {
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        if !data.automation.pending_captures.contains_key(&id) {
            return Ok(());
        }
        let mut next = data.automation.clone();
        next.pending_captures.remove(&id);
        next.membership.remove(&id);
        let downloads: Vec<_> = data
            .downloads
            .iter()
            .filter(|d| d.id != id)
            .cloned()
            .collect();
        self.inner
            .store
            .discard_capture(id, &next, &downloads)
            .await?;
        data.automation = next;
        data.downloads = downloads;
        self.inner.captured_headers.lock().await.remove(&id);
        Ok(())
    }

    pub async fn set_captured_headers(
        &self,
        id: DownloadId,
        headers: crate::CapturedHeaders,
    ) -> Result<()> {
        for value in [&headers.cookie, &headers.authorization, &headers.user_agent]
            .into_iter()
            .flatten()
        {
            if value.len() > 16384 || reqwest::header::HeaderValue::from_str(value).is_err() {
                return Err(Error::Invalid("invalid captured header".into()));
            }
        }
        let data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        let download = data
            .downloads
            .iter()
            .find(|d| d.id == id)
            .ok_or(Error::NotFound)?;
        if data.running.contains_key(&id) || download.state != DownloadState::Paused {
            return Err(Error::Invalid(
                "captured credentials require a paused transfer".into(),
            ));
        }
        let url =
            reqwest::Url::parse(&download.url).map_err(|_| Error::Invalid("invalid URL".into()))?;
        if url.scheme() != "https"
            && !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        {
            return Err(Error::Invalid("captured credentials require HTTPS".into()));
        }
        self.inner
            .captured_headers
            .lock()
            .await
            .insert(id, (url.origin().ascii_serialization(), headers));
        Ok(())
    }

    pub async fn enqueue_batch(
        &self,
        items: Vec<BatchItem>,
        start_paused: bool,
    ) -> Result<Vec<BatchResult>> {
        if items.is_empty() || items.len() > 1000 {
            return Err(Error::Invalid(
                "batch must contain 1 to 1000 requests".into(),
            ));
        }
        let mut results = Vec::new();
        for (index, item) in items.into_iter().enumerate() {
            let mut result = BatchResult {
                index,
                id: None,
                duplicate: false,
                error: None,
            };
            let admission: Result<DownloadId> = async {
                if let Some(queue) = item.queue_id
                    && !self.automation().await.queues.iter().any(|q| q.id == queue)
                {
                    return Err(Error::NotFound);
                }
                let mut request = item.request;
                request.expected_sha256 = crate::integrity::normalize(request.expected_sha256)?;
                if !request.destination.is_absolute() || request.destination.file_name().is_none() {
                    return Err(Error::Invalid(
                        "destination must be an absolute filename".into(),
                    ));
                }
                let mut url = reqwest::Url::parse(&request.url)
                    .map_err(|_| Error::Invalid("invalid URL".into()))?;
                url.set_fragment(None);
                request.url = url.to_string();
                let parent = request
                    .destination
                    .parent()
                    .ok_or_else(|| Error::Invalid("invalid destination".into()))?;
                let parent = tokio::fs::canonicalize(parent).await?;
                let file = request
                    .destination
                    .file_name()
                    .ok_or_else(|| Error::Invalid("invalid destination".into()))?;
                request.destination = parent.join(file);
                let old = self.snapshot().await.into_iter().find(|d| {
                    d.url == request.url
                        && crate::engine::same_destination(&d.destination, &request.destination)
                        && d.expected_sha256 == request.expected_sha256
                });
                let id = if let Some(old) = old {
                    result.duplicate = true;
                    old.id
                } else {
                    self.enqueue_paused(request).await?
                };
                // Preserve identity even if a subsequent assignment/resume fails.
                result.id = Some(id);
                if let Some(queue) = item.queue_id {
                    let current = self.automation().await;
                    if current.membership.get(&id) != Some(&queue) {
                        self.configure_automation(AutomationCommand::Assign {
                            ids: vec![id],
                            queue_id: Some(queue),
                        })
                        .await?;
                    }
                }
                if !start_paused {
                    self.resume(id).await?;
                }
                Ok(id)
            }
            .await;
            if admission.is_err() {
                result.error = Some("admission_failed".into());
            }
            results.push(result);
        }
        Ok(results)
    }

    pub async fn automation(&self) -> Automation {
        self.inner.data.lock().await.automation.clone()
    }

    pub async fn configure_automation(&self, command: AutomationCommand) -> Result<Automation> {
        self.apply_automation(Some(command), unix_now(), false)
            .await
    }

    /// Explicit clock is the test seam; production ticks supply UTC seconds.
    pub async fn automation_tick(&self, now: u64, startup: bool) -> Result<Automation> {
        let expired: Vec<_> = self
            .automation()
            .await
            .pending_captures
            .into_iter()
            .filter(|(_, until)| *until <= now)
            .map(|(id, _)| id)
            .collect();
        for id in expired {
            self.abort_capture(id).await?;
        }
        self.apply_automation(None, now, startup).await
    }

    async fn apply_automation(
        &self,
        command: Option<AutomationCommand>,
        now: u64,
        startup: bool,
    ) -> Result<Automation> {
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        let mut next = data.automation.clone();
        let mut downloads = data.downloads.clone();
        next.membership
            .retain(|id, _| downloads.iter().any(|d| d.id == *id));
        let mut starts = HashMap::new();
        let mut stops = HashSet::new();
        if let Some(command) = command {
            match command {
                AutomationCommand::SaveQueue { id, name } => {
                    if let Some(q) = next.queues.iter_mut().find(|q| q.id == id) {
                        q.name = name.trim().into();
                    } else {
                        next.queues.push(DownloadQueue {
                            id,
                            name: name.trim().into(),
                            running: false,
                            stop_at: None,
                            synchronize: false,
                            checking_sources: false,
                            last_error: None,
                        });
                    }
                }
                AutomationCommand::SetSynchronization { id, enabled } => {
                    if queue_has_workers(&data, id) {
                        return Err(Error::Invalid(
                            "wait for queue workers to stop before changing synchronization policy"
                                .into(),
                        ));
                    }
                    let queue = next
                        .queues
                        .iter_mut()
                        .find(|q| q.id == id)
                        .ok_or(Error::NotFound)?;
                    if queue.running {
                        return Err(Error::Invalid(
                            "stop queue before changing synchronization policy".into(),
                        ));
                    }
                    queue.synchronize = enabled;
                }
                AutomationCommand::DeleteQueue { id } => {
                    if next.queues.iter().any(|q| q.id == id && q.running)
                        || queue_has_workers(&data, id)
                    {
                        return Err(Error::Invalid("stop queue before deletion".into()));
                    }
                    next.queues.retain(|q| q.id != id);
                    next.schedules.retain(|s| s.queue_id != id);
                    next.membership.retain(|_, q| *q != id);
                }
                AutomationCommand::Assign { ids, queue_id } => {
                    for id in ids {
                        let d = downloads
                            .iter()
                            .find(|d| d.id == id)
                            .ok_or(Error::NotFound)?;
                        if data.running.contains_key(&id)
                            || matches!(
                                d.state,
                                DownloadState::Queued
                                    | DownloadState::Pausing
                                    | DownloadState::Canceling
                            )
                        {
                            return Err(Error::Invalid(
                                "pause downloads before assigning a queue".into(),
                            ));
                        }
                        if let Some(q) = queue_id {
                            next.membership.insert(id, q);
                        } else {
                            next.membership.remove(&id);
                        }
                    }
                }
                AutomationCommand::SaveSchedule { schedule } => {
                    if let Some(s) = next.schedules.iter_mut().find(|s| s.id == schedule.id) {
                        *s = schedule;
                    } else {
                        next.schedules.push(schedule);
                    }
                }
                AutomationCommand::DeleteSchedule { id } => next.schedules.retain(|s| s.id != id),
                AutomationCommand::Start { id } => {
                    if queue_has_workers(&data, id)
                        && !next.queues.iter().any(|q| q.id == id && q.running)
                    {
                        return Err(Error::Invalid(
                            "queue is still checking or stopping; wait before starting it again"
                                .into(),
                        ));
                    }
                    starts.insert(id, None);
                }
                AutomationCommand::Stop { id } => {
                    stops.insert(id);
                }
            }
        } else {
            for s in next
                .schedules
                .iter_mut()
                .filter(|_| self.inner.schedules_enabled)
            {
                if !s.enabled || s.next_run > now {
                    continue;
                }
                let late = now - s.next_run;
                // Missed slots coalesce to at most one run, never a catch-up storm.
                if (!startup && late <= 30) || (startup && s.run_once_on_startup) {
                    starts.entry(s.queue_id).or_insert(s.stop_after_seconds);
                }
                if let Some(interval) = s.interval_seconds {
                    s.next_run = s
                        .next_run
                        .saturating_add((late / interval + 1).saturating_mul(interval));
                } else {
                    s.enabled = false;
                }
            }
            for q in &next.queues {
                if q.stop_at.is_some_and(|end| now >= end) {
                    stops.insert(q.id);
                }
            }
        }
        next.validate()?;
        // A later slot never restarts a manually paused member of an overlapping run.
        starts.retain(|id, _| {
            !queue_has_workers(&data, *id) && !next.queues.iter().any(|q| q.id == *id && q.running)
        });
        for id in starts.keys().chain(stops.iter()) {
            if !next.queues.iter().any(|q| q.id == *id) {
                return Err(Error::NotFound);
            }
        }
        for q in &mut next.queues {
            if stops.contains(&q.id) {
                q.running = false;
                q.stop_at = None;
                q.checking_sources = false;
            } else if let Some(window) = starts.get(&q.id) {
                q.running = true;
                q.checking_sources = q.synchronize;
                q.last_error = None;
                q.stop_at = window.map(|n| now.saturating_add(n));
            }
        }
        let mut drain = Vec::new();
        for d in &mut downloads {
            if next.pending_captures.contains_key(&d.id) {
                continue;
            }
            let Some(queue) = next.membership.get(&d.id) else {
                continue;
            };
            if stops.contains(queue)
                && matches!(
                    d.state,
                    DownloadState::Queued | DownloadState::Downloading | DownloadState::Pausing
                )
            {
                if data.running.contains_key(&d.id) {
                    d.state = DownloadState::Pausing;
                    drain.push(d.id);
                } else {
                    d.state = DownloadState::Paused;
                }
            } else if starts.contains_key(queue)
                && !stops.contains(queue)
                && d.state == DownloadState::Paused
            {
                d.state = DownloadState::Queued;
                d.error = None;
            }
        }
        for q in &mut next.queues {
            if q.running
                && !q.checking_sources
                && !downloads.iter().any(|d| {
                    next.membership.get(&d.id) == Some(&q.id)
                        && matches!(
                            d.state,
                            DownloadState::Queued
                                | DownloadState::Downloading
                                | DownloadState::Pausing
                        )
                })
            {
                q.running = false;
                q.stop_at = None;
            }
        }
        if serde_json::to_string(&next)? != serde_json::to_string(&data.automation)?
            || !starts.is_empty()
            || !stops.is_empty()
        {
            // Intent and all selected states are one transaction. Failed writes
            // leave live state and cancellation tokens unchanged.
            self.inner.store.save_automation(&next, &downloads).await?;
            data.automation = next;
            data.downloads = downloads;
            for id in &drain {
                if let Some(token) = data.running.get(id) {
                    token.cancel();
                }
            }
            for id in &stops {
                if let Some(token) = data.synchronizing.get(id) {
                    token.cancel();
                }
            }
        }
        let check_queues: Vec<_> = data
            .automation
            .queues
            .iter()
            .filter(|q| q.running && q.checking_sources && !data.synchronizing.contains_key(&q.id))
            .map(|q| q.id)
            .collect();
        for queue in check_queues {
            let token = tokio_util::sync::CancellationToken::new();
            data.synchronizing.insert(queue, token.clone());
            let engine = self.clone();
            tokio::spawn(async move {
                let worker = engine.clone();
                let result =
                    tokio::spawn(async move { worker.check_sync_sources(queue, token).await })
                        .await;
                let mut data = engine.inner.data.lock().await;
                let mut next = data.automation.clone();
                if let Some(q) = next.queues.iter_mut().find(|q| q.id == queue) {
                    q.checking_sources = false;
                    if !matches!(result, Ok(Ok(())) | Ok(Err(Error::Interrupted))) {
                        q.last_error = Some("Source check failed; existing files are retained. Run queue again to retry.".into());
                    }
                }
                if engine
                    .inner
                    .store
                    .save_automation(&next, &data.downloads)
                    .await
                    .is_ok()
                {
                    data.automation = next;
                } else {
                    let _ = engine.inner.events.send(crate::EngineEvent::Error(
                        "Could not save synchronization result".into(),
                    ));
                }
                data.synchronizing.remove(&queue);
            });
        }
        let result = data.automation.clone();
        drop(data);
        for id in drain {
            self.wait_idle(id).await;
        }
        for id in stops {
            self.wait_sync_idle(id).await;
        }
        Ok(result)
    }

    async fn check_sync_sources(
        &self,
        queue: DownloadId,
        token: tokio_util::sync::CancellationToken,
    ) -> Result<()> {
        let data = self.inner.data.lock().await;
        let roots: Vec<_> = data
            .downloads
            .iter()
            .filter(|d| {
                data.automation.membership.get(&d.id) == Some(&queue)
                    && !data.automation.sync_roots.contains_key(&d.id)
            })
            .map(|d| d.id)
            .collect();
        let baselines: Vec<_> = roots
            .into_iter()
            .filter_map(|root| {
                // A pending revision already represents this source; do not spawn a
                // second transfer simply because another interval elapsed.
                let revisions: Vec<_> = data
                    .downloads
                    .iter()
                    .filter(|d| {
                        d.id == root || data.automation.sync_roots.get(&d.id) == Some(&root)
                    })
                    .collect();
                if revisions
                    .iter()
                    .any(|d| !matches!(d.state, DownloadState::Completed | DownloadState::Canceled))
                {
                    return None;
                }
                revisions
                    .into_iter()
                    .rev()
                    .find(|d| d.state == DownloadState::Completed)
                    .map(|d| (root, d.clone()))
            })
            .collect();
        drop(data);
        for (root, baseline) in baselines {
            if token.is_cancelled() {
                return Err(Error::Interrupted);
            }
            let probe = crate::transfer::probe(&self.inner, &baseline.url, &token).await?;
            let local_present = tokio::fs::symlink_metadata(&baseline.destination)
                .await
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink());
            if local_present
                && probe.validator.is_some()
                && probe.validator == baseline.validator
                && probe.length.is_some()
                && probe.length == baseline.total_bytes
            {
                continue;
            }
            let original = self
                .snapshot()
                .await
                .into_iter()
                .find(|d| d.id == root)
                .ok_or(Error::NotFound)?;
            let stem = original
                .destination
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("download");
            let mut stem = stem.to_string();
            while stem.len() > 160 {
                stem.pop();
            }
            let extension = original
                .destination
                .extension()
                .and_then(|n| n.to_str())
                .filter(|s| s.len() <= 24)
                .map(|s| format!(".{s}"))
                .unwrap_or_default();
            let name = format!(
                "{stem}.revision-{}{extension}",
                DownloadId::new_v4().simple()
            );
            let request = crate::DownloadRequest {
                url: baseline.url,
                destination: original.destination.with_file_name(name),
                expected_sha256: None,
            };
            self.enqueue_sync_revision(request, original.properties, queue, root, token.clone())
                .await?;
        }
        Ok(())
    }
}
