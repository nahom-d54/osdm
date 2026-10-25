use crate::{store::Store, transfer, *};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Semaphore, broadcast};
use tokio_util::sync::CancellationToken;

/// Windows compares destination names without regard to case. Compare the reserved
/// destination the same way so `File.bin` and `file.bin` cannot both enter the queue.
fn same_destination(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    #[cfg(windows)]
    {
        left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn validated_url(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw).map_err(|_| Error::Invalid("invalid URL".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::Invalid(
            "use an HTTP(S) URL without embedded credentials".into(),
        ));
    }
    Ok(url)
}

#[derive(Clone)]
pub struct Engine {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Data {
    pub downloads: Vec<Download>,
    pub running: HashMap<DownloadId, CancellationToken>,
    pub stopped: bool,
}

pub(crate) struct Inner {
    database_lock: Mutex<Option<std::fs::File>>,
    pub data: Mutex<Data>,
    pub store: Store,
    pub config: Config,
    pub client: reqwest::Client,
    pub connections: Arc<Semaphore>,
    pub hosts: Mutex<HashMap<String, Arc<adaptive::Host>>>,
    pub download_hosts: Mutex<HashMap<DownloadId, Arc<adaptive::Host>>>,
    pub cooldowns: retry::Cooldowns,
    pub rate: AtomicU64,
    pub rate_clock: Mutex<tokio::time::Instant>,
    pub rate_changed: tokio::sync::Notify,
    pub events: broadcast::Sender<EngineEvent>,
}

impl Inner {
    pub async fn host(&self, url: &str) -> Result<Arc<adaptive::Host>> {
        let url = validated_url(url)?;
        let hostname = url.host_str().unwrap().to_owned();
        Ok(self
            .hosts
            .lock()
            .await
            .entry(hostname)
            .or_insert_with(|| {
                Arc::new(adaptive::Host::new(
                    self.config
                        .max_connections_per_host
                        .min(self.config.max_connections),
                ))
            })
            .clone())
    }
    /// Call with the data lock held so persisted snapshots cannot overtake each other.
    pub async fn save(&self, data: &Data, id: DownloadId) -> Result<()> {
        let position = data
            .downloads
            .iter()
            .position(|d| d.id == id)
            .ok_or(Error::NotFound)?;
        self.store.save(&data.downloads[position], position).await
    }
}

impl Engine {
    /// Requires a Tokio runtime. Use one engine per database and call shutdown before reopening.
    pub async fn open(database: impl AsRef<Path>, config: Config) -> Result<Self> {
        config.validate()?;
        let database = if database.as_ref().exists() {
            std::fs::canonicalize(database.as_ref())?
        } else {
            let absolute = std::path::absolute(database.as_ref())?;
            std::fs::canonicalize(
                absolute
                    .parent()
                    .ok_or_else(|| Error::Invalid("invalid database path".into()))?,
            )?
            .join(
                absolute
                    .file_name()
                    .ok_or_else(|| Error::Invalid("invalid database path".into()))?,
            )
        };
        let mut lock_path = database.as_os_str().to_owned();
        lock_path.push(".lock");
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        lock.try_lock().map_err(|_| {
            Error::Invalid("database is already owned by another engine or cannot be locked".into())
        })?;
        let store = Store::open(&database).await?;
        let mut downloads = store.load().await?;
        for (position, download) in downloads.iter_mut().enumerate() {
            if matches!(
                download.state,
                DownloadState::Downloading | DownloadState::Pausing | DownloadState::Queued
            ) {
                download.state = DownloadState::Paused;
                download.error = None;
            } else if download.state == DownloadState::Canceling {
                download.state = DownloadState::Canceled;
            }
            download.bytes_per_second = 0;
            if download.integrity == IntegrityStatus::Verifying {
                download.integrity = IntegrityStatus::Unverified;
            }
            download.eta_seconds = None;
            for segment in &mut download.segments {
                segment.downloaded = segment.committed;
            }
            store.save(download, position).await?;
        }
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent("OSDM/0.1")
            .build()?;
        let (events, _) = broadcast::channel(16);
        let inner = Arc::new(Inner {
            database_lock: Mutex::new(Some(lock)),
            connections: Arc::new(Semaphore::new(config.max_connections)),
            rate: AtomicU64::new(config.bytes_per_second),
            rate_clock: Mutex::new(tokio::time::Instant::now()),
            rate_changed: tokio::sync::Notify::new(),
            hosts: Mutex::new(HashMap::new()),
            download_hosts: Mutex::new(HashMap::new()),
            cooldowns: retry::Cooldowns::default(),
            data: Mutex::new(Data {
                downloads,
                running: HashMap::new(),
                stopped: false,
            }),
            store,
            config,
            client,
            events,
        });
        let weak = Arc::downgrade(&inner);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut samples = HashMap::<DownloadId, (Instant, u64)>::new();
            loop {
                tick.tick().await;
                let Some(inner) = weak.upgrade() else { break };
                {
                    let hosts = inner.hosts.lock().await;
                    for host in hosts.values() {
                        host.tick(inner.rate.load(Ordering::Relaxed) > 0);
                    }
                }
                let mut data = inner.data.lock().await;
                if data.stopped {
                    break;
                }
                let now = Instant::now();
                let download_hosts = inner.download_hosts.lock().await;
                for download in &mut data.downloads {
                    download.adaptive = download_hosts
                        .get(&download.id)
                        .map(|host| host.snapshot(download.id));
                    let bytes = download.downloaded_bytes();
                    let (previous, old_bytes) = samples
                        .insert(download.id, (now, bytes))
                        .unwrap_or((now, bytes));
                    let elapsed = now.duration_since(previous).as_secs_f64();
                    download.bytes_per_second =
                        if download.state == DownloadState::Downloading && elapsed > 0.0 {
                            (bytes.saturating_sub(old_bytes) as f64 / elapsed) as u64
                        } else {
                            0
                        };
                    download.eta_seconds = download.total_bytes.and_then(|total| {
                        (download.bytes_per_second > 0)
                            .then(|| total.saturating_sub(bytes) / download.bytes_per_second)
                    });
                }
                drop(download_hosts);
                let capacity = inner
                    .config
                    .max_active_downloads
                    .saturating_sub(data.running.len());
                let ids: Vec<_> = data
                    .downloads
                    .iter()
                    .filter(|d| d.state == DownloadState::Queued)
                    .take(capacity)
                    .map(|d| d.id)
                    .collect();
                for id in ids {
                    let download = data.downloads.iter_mut().find(|d| d.id == id).unwrap();
                    download.state = DownloadState::Downloading;
                    if let Err(error) = inner.save(&data, id).await {
                        let download = data.downloads.iter_mut().find(|d| d.id == id).unwrap();
                        download.state = DownloadState::Failed;
                        download.error = Some(error.to_string());
                        let _ = inner.events.send(EngineEvent::Error(error.to_string()));
                        continue;
                    }
                    let token = CancellationToken::new();
                    data.running.insert(id, token.clone());
                    tokio::spawn(run(inner.clone(), id, token));
                }
                let _ = inner
                    .events
                    .send(EngineEvent::Snapshot(data.downloads.clone()));
            }
        });
        Ok(Self { inner })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.events.subscribe()
    }

    pub async fn snapshot(&self) -> Vec<Download> {
        self.inner.data.lock().await.downloads.clone()
    }

    /// Read response metadata without creating a queue entry. The desktop
    /// workflow uses this to propose a real server filename before enqueueing.
    pub async fn probe(&self, url: &str) -> Result<ProbeInfo> {
        {
            let data = self.inner.data.lock().await;
            if data.stopped {
                return Err(Error::Shutdown);
            }
        }
        let url = validated_url(url)?;
        let token = CancellationToken::new();
        let probe = transfer::probe(&self.inner, url.as_str(), &token).await?;
        Ok(ProbeInfo {
            final_url: probe.url,
            filename: probe.filename,
            content_type: probe.content_type,
            total_bytes: probe.length,
            ranged: probe.ranged,
        })
    }

    pub async fn enqueue(&self, request: DownloadRequest) -> Result<DownloadId> {
        let url = validated_url(&request.url)?;
        let expected_sha256 = integrity::normalize(request.expected_sha256)?;
        if !request.destination.is_absolute() || request.destination.file_name().is_none() {
            return Err(Error::Invalid(
                "destination must be an absolute filename".into(),
            ));
        }
        let parent = request.destination.parent().unwrap();
        let parent = tokio::fs::canonicalize(parent).await?;
        let destination = parent.join(request.destination.file_name().unwrap());
        if tokio::fs::try_exists(&destination).await? {
            return Err(Error::Invalid("destination already exists".into()));
        }
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        if data.downloads.iter().any(|d| {
            same_destination(&d.destination, &destination) && d.state != DownloadState::Completed
        }) {
            return Err(Error::Invalid(
                "destination is already reserved by a download".into(),
            ));
        }
        let id = DownloadId::new_v4();
        let download = Download {
            id,
            url: url.to_string(),
            temporary_path: parent.join(format!(".osdm-{id}.part")),
            destination,
            state: DownloadState::Queued,
            total_bytes: None,
            segments: vec![],
            error: None,
            bytes_per_second: 0,
            eta_seconds: None,
            adaptive: None,
            expected_sha256,
            sha256: None,
            integrity: IntegrityStatus::Unverified,
            validator: None,
            ranged: false,
            ready_to_publish: false,
        };
        self.inner
            .store
            .save(&download, data.downloads.len())
            .await?;
        data.downloads.push(download);
        Ok(id)
    }

    pub async fn pause(&self, id: DownloadId) -> Result<()> {
        self.stop(id, false).await
    }
    pub async fn cancel(&self, id: DownloadId) -> Result<()> {
        self.stop(id, true).await
    }

    async fn stop(&self, id: DownloadId, cancel: bool) -> Result<()> {
        {
            let mut data = self.inner.data.lock().await;
            if data.stopped {
                return Err(Error::Shutdown);
            }
            let token = data.running.get(&id).cloned();
            let download = data
                .downloads
                .iter_mut()
                .find(|d| d.id == id)
                .ok_or(Error::NotFound)?;
            if download.state == DownloadState::Completed {
                return Ok(());
            }
            download.state = match (cancel, token.is_some()) {
                (true, true) => DownloadState::Canceling,
                (true, false) => DownloadState::Canceled,
                (false, true) => DownloadState::Pausing,
                (false, false) => DownloadState::Paused,
            };
            if let Some(token) = token {
                token.cancel();
            }
            self.inner.save(&data, id).await?;
        }
        self.wait_idle(id).await;
        Ok(())
    }

    async fn wait_idle(&self, id: DownloadId) {
        while self.inner.data.lock().await.running.contains_key(&id) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub async fn resume(&self, id: DownloadId) -> Result<()> {
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        let download = data
            .downloads
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(Error::NotFound)?;
        if matches!(
            download.state,
            DownloadState::Paused | DownloadState::Failed | DownloadState::Canceled
        ) {
            download.state = DownloadState::Queued;
            download.error = None;
            self.inner.save(&data, id).await?;
        }
        Ok(())
    }

    pub async fn retry(&self, id: DownloadId) -> Result<()> {
        self.resume(id).await
    }

    /// Replace an inactive source. Resume revalidates the representation before
    /// reusing checkpoints; a changed file starts over rather than mixing bytes.
    pub async fn update_url(&self, id: DownloadId, url: &str) -> Result<()> {
        let url = validated_url(url.trim())?;
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        let position = data
            .downloads
            .iter()
            .position(|d| d.id == id)
            .ok_or(Error::NotFound)?;
        let mut download = data.downloads[position].clone();
        if data.running.contains_key(&id)
            || !matches!(
                download.state,
                DownloadState::Paused | DownloadState::Failed
            )
        {
            return Err(Error::Invalid(
                "pause the download before changing its URL".into(),
            ));
        }
        // A failed completion save can leave the temporary file hard-linked to
        // the published destination. Revalidation must never truncate that inode.
        if download.ready_to_publish && tokio::fs::try_exists(&download.destination).await? {
            return Err(Error::Invalid("the file is already awaiting publication recovery; resume it before changing the source".into()));
        }
        download.url = url.to_string();
        download.state = DownloadState::Paused;
        download.error = None;
        download.sha256 = None;
        // Keep the mismatch marker until prepare discards suspect checkpoints.
        if download.integrity != IntegrityStatus::Mismatch {
            download.integrity = IntegrityStatus::Unverified;
        }
        download.ready_to_publish = false;
        self.inner.store.save(&download, position).await?;
        data.downloads[position] = download;
        Ok(())
    }

    /// Supply every known download ID exactly once. Active transfers are not interrupted.
    pub async fn reorder(&self, ids: &[DownloadId]) -> Result<()> {
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        let actual: HashSet<_> = data.downloads.iter().map(|d| d.id).collect();
        if ids.len() != actual.len() || ids.iter().copied().collect::<HashSet<_>>() != actual {
            return Err(Error::Invalid(
                "reorder must include every download exactly once".into(),
            ));
        }
        let reordered: Vec<_> = ids
            .iter()
            .map(|id| data.downloads.iter().find(|d| d.id == *id).unwrap().clone())
            .collect();
        self.inner.store.reorder(&reordered).await?;
        data.downloads = reordered;
        Ok(())
    }

    pub async fn set_speed_limit(&self, bytes_per_second: u64) {
        let mut clock = self.inner.rate_clock.lock().await;
        self.inner.rate.store(bytes_per_second, Ordering::Relaxed);
        *clock = tokio::time::Instant::now();
        self.inner.rate_changed.notify_waiters();
    }

    /// Pauses all work and waits for workers to drain. This engine cannot be restarted.
    pub async fn shutdown(&self) -> Result<()> {
        let ids = {
            let mut data = self.inner.data.lock().await;
            data.stopped = true;
            let ids: Vec<_> = data.running.keys().copied().collect();
            for token in data.running.values() {
                token.cancel();
            }
            for download in &mut data.downloads {
                if matches!(
                    download.state,
                    DownloadState::Queued | DownloadState::Downloading
                ) {
                    download.state = if ids.contains(&download.id) {
                        DownloadState::Pausing
                    } else {
                        DownloadState::Paused
                    };
                }
            }
            ids
        };
        for id in ids {
            self.wait_idle(id).await;
        }
        let data = self.inner.data.lock().await;
        let mut ownership = self.inner.database_lock.lock().await;
        if ownership.is_none() {
            return Ok(());
        }
        for (position, download) in data.downloads.iter().enumerate() {
            self.inner.store.save(download, position).await?;
        }
        ownership.take();
        Ok(())
    }
}

async fn run(inner: Arc<Inner>, id: DownloadId, token: CancellationToken) {
    // Join an isolated task so even a panic cannot leave a permanently occupied queue slot.
    let worker_inner = inner.clone();
    let result = tokio::spawn(async move { transfer::download(worker_inner, id, token).await })
        .await
        .unwrap_or_else(|_| {
            Err(Error::Invalid(
                "download worker terminated unexpectedly".into(),
            ))
        });
    let mut data = inner.data.lock().await;
    let download = data.downloads.iter_mut().find(|d| d.id == id).unwrap();
    if download.state != DownloadState::Completed {
        download.state = match download.state {
            DownloadState::Pausing => DownloadState::Paused,
            DownloadState::Canceling => DownloadState::Canceled,
            _ => DownloadState::Failed,
        };
        if let Err(error) = result
            && !matches!(error, Error::Interrupted)
        {
            download.error = Some(error.to_string());
        }
        for segment in &mut download.segments {
            segment.downloaded = segment.committed;
        }
    }
    download.bytes_per_second = 0;
    download.eta_seconds = None;
    if let Err(error) = inner.save(&data, id).await {
        let _ = inner.events.send(EngineEvent::Error(error.to_string()));
    }
    data.running.remove(&id);
    inner.download_hosts.lock().await.remove(&id);
}
