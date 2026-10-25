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
pub(crate) fn same_destination(left: &Path, right: &Path) -> bool {
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

struct Admission {
    properties: DownloadProperties,
    start_paused: bool,
    credentials: Option<BasicCredentials>,
    temporary_directory: Option<std::path::PathBuf>,
    capture: bool,
    sync: Option<(DownloadId, DownloadId, CancellationToken)>,
}

fn validate_credentials(url: &reqwest::Url, credentials: Option<&BasicCredentials>) -> Result<()> {
    if let Some(credentials) = credentials {
        if credentials.username.len() > 1024
            || credentials.password.len() > 4096
            || credentials.username.contains(':')
            || credentials.username.contains(['\r', '\n'])
            || credentials.password.contains(['\r', '\n'])
        {
            return Err(Error::Invalid(
                "invalid Basic authentication credentials".into(),
            ));
        }
        if url.scheme() != "https"
            && !matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"))
        {
            return Err(Error::Invalid(
                "credentials require HTTPS (except loopback testing)".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) struct Data {
    pub downloads: Vec<Download>,
    pub automation: Automation,
    pub running: HashMap<DownloadId, CancellationToken>,
    pub synchronizing: HashMap<DownloadId, CancellationToken>,
    pub stopped: bool,
}

pub(crate) struct Inner {
    database_lock: Mutex<Option<std::fs::File>>,
    pub data: Mutex<Data>,
    pub store: Store,
    pub config: Config,
    pub schedules_enabled: bool,
    pub client: reqwest::Client,
    pub credentials: Mutex<HashMap<DownloadId, (String, BasicCredentials)>>,
    pub captured_headers: Mutex<HashMap<DownloadId, (String, CapturedHeaders)>>,
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
    pub async fn request(&self, url: &str, id: Option<DownloadId>) -> reqwest::RequestBuilder {
        let mut request = self.client.get(url);
        let headers = self.captured_headers.lock().await;
        if let Some((origin, secret)) = id.and_then(|id| headers.get(&id))
            && reqwest::Url::parse(url)
                .is_ok_and(|url| url.origin().ascii_serialization() == *origin)
        {
            if let Some(value) = &secret.cookie {
                request = request.header(reqwest::header::COOKIE, value);
            }
            if let Some(value) = &secret.authorization {
                request = request.header(reqwest::header::AUTHORIZATION, value);
            }
            if let Some(value) = &secret.user_agent {
                request = request.header(reqwest::header::USER_AGENT, value);
            }
        }
        drop(headers);
        let credentials = self.credentials.lock().await;
        if let Some((origin, secret)) = id.and_then(|id| credentials.get(&id))
            && reqwest::Url::parse(url)
                .is_ok_and(|url| url.origin().ascii_serialization() == *origin)
        {
            return request.basic_auth(&secret.username, Some(&secret.password));
        }
        request
    }
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
        Self::open_inner(database, config, true).await
    }

    /// Short-lived command owners perform housekeeping but do not dispatch saved
    /// schedules merely because a script listed or added a deferred request.
    pub async fn open_for_commands(database: impl AsRef<Path>, config: Config) -> Result<Self> {
        Self::open_inner(database, config, false).await
    }

    async fn open_inner(
        database: impl AsRef<Path>,
        config: Config,
        schedules_enabled: bool,
    ) -> Result<Self> {
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
        let mut automation = store.load_automation().await?;
        automation.validate()?;
        // Recovery never restarts transfers merely because a queue was running.
        for queue in &mut automation.queues {
            queue.running = false;
            queue.checking_sources = false;
            queue.stop_at = None;
        }
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
                automation,
                running: HashMap::new(),
                synchronizing: HashMap::new(),
                stopped: false,
            }),
            store,
            config,
            schedules_enabled,
            client,
            credentials: Mutex::new(HashMap::new()),
            captured_headers: Mutex::new(HashMap::new()),
            events,
        });
        let weak = Arc::downgrade(&inner);
        let automation_weak = weak.clone();
        tokio::spawn(async move {
            let mut startup = true;
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let Some(inner) = automation_weak.upgrade() else {
                    break;
                };
                if inner.data.lock().await.stopped {
                    break;
                }
                let engine = Engine { inner };
                if let Err(error) = engine
                    .automation_tick(crate::automation::unix_now(), startup)
                    .await
                {
                    let _ = engine
                        .inner
                        .events
                        .send(EngineEvent::Error(error.to_string()));
                }
                startup = false;
            }
        });
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
        self.enqueue_with_properties(request, DownloadProperties::default(), false, None, None)
            .await
    }

    /// Persist a deferred download without admitting it to the scheduler.
    pub async fn enqueue_paused(&self, request: DownloadRequest) -> Result<DownloadId> {
        self.enqueue_with_properties(request, DownloadProperties::default(), true, None, None)
            .await
    }

    pub async fn enqueue_with_properties(
        &self,
        request: DownloadRequest,
        properties: DownloadProperties,
        start_paused: bool,
        credentials: Option<BasicCredentials>,
        temporary_directory: Option<std::path::PathBuf>,
    ) -> Result<DownloadId> {
        self.admit(
            request,
            Admission {
                properties,
                start_paused,
                credentials,
                temporary_directory,
                capture: false,
                sync: None,
            },
        )
        .await
    }

    pub async fn prepare_capture(&self, request: DownloadRequest) -> Result<DownloadId> {
        self.admit(
            request,
            Admission {
                properties: DownloadProperties::default(),
                start_paused: true,
                credentials: None,
                temporary_directory: None,
                capture: true,
                sync: None,
            },
        )
        .await
    }

    pub(crate) async fn enqueue_sync_revision(
        &self,
        request: DownloadRequest,
        properties: DownloadProperties,
        queue: DownloadId,
        root: DownloadId,
        token: CancellationToken,
    ) -> Result<DownloadId> {
        self.admit(
            request,
            Admission {
                properties,
                start_paused: false,
                credentials: None,
                temporary_directory: None,
                capture: false,
                sync: Some((queue, root, token)),
            },
        )
        .await
    }

    async fn admit(&self, request: DownloadRequest, admission: Admission) -> Result<DownloadId> {
        let Admission {
            properties,
            start_paused,
            credentials,
            temporary_directory,
            capture,
            sync,
        } = admission;
        properties.validate()?;
        let url = validated_url(&request.url)?;
        validate_credentials(&url, credentials.as_ref())?;
        let expected_sha256 = integrity::normalize(request.expected_sha256)?;
        if !request.destination.is_absolute() || request.destination.file_name().is_none() {
            return Err(Error::Invalid(
                "destination must be an absolute filename".into(),
            ));
        }
        let parent = request.destination.parent().unwrap();
        let parent = tokio::fs::canonicalize(parent).await?;
        let destination = parent.join(request.destination.file_name().unwrap());
        let temporary_directory = match temporary_directory {
            Some(directory) => Some(storage::validate_directory(directory, &parent).await?),
            None => None,
        };
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
            temporary_path: temporary_directory
                .as_ref()
                .unwrap_or(&parent)
                .join(format!(".osdm-{id}.part")),
            temporary_directory,
            destination,
            properties,
            state: if start_paused {
                DownloadState::Paused
            } else {
                DownloadState::Queued
            },
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
        if capture || sync.is_some() {
            let mut next = data.automation.clone();
            if capture {
                next.pending_captures
                    .insert(id, crate::automation::unix_now().saturating_add(30));
            }
            if let Some((queue, root, token)) = sync {
                if token.is_cancelled()
                    || next.membership.get(&root) != Some(&queue)
                    || !next.queues.iter().any(|q| q.id == queue && q.running)
                {
                    return Err(Error::Interrupted);
                }
                next.membership.insert(id, queue);
                next.sync_roots.insert(id, root);
            }
            self.inner
                .store
                .save_capture(&download, data.downloads.len(), &next)
                .await?;
            data.automation = next;
        } else {
            self.inner
                .store
                .save(&download, data.downloads.len())
                .await?;
        }
        data.downloads.push(download);
        if let Some(credentials) = credentials {
            self.inner
                .credentials
                .lock()
                .await
                .insert(id, (url.origin().ascii_serialization(), credentials));
        }
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
            if cancel {
                self.inner.credentials.lock().await.remove(&id);
                self.inner.captured_headers.lock().await.remove(&id);
            }
        }
        self.wait_idle(id).await;
        Ok(())
    }

    pub(crate) async fn wait_idle(&self, id: DownloadId) {
        while self.inner.data.lock().await.running.contains_key(&id) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub(crate) async fn wait_sync_idle(&self, id: DownloadId) {
        while self.inner.data.lock().await.synchronizing.contains_key(&id) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub async fn resume(&self, id: DownloadId) -> Result<()> {
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        if data.automation.pending_captures.contains_key(&id) {
            return Err(Error::Invalid(
                "browser capture awaits handoff confirmation".into(),
            ));
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

    /// Organization changes never move files or alter transfer identity.
    pub async fn update_properties(
        &self,
        id: DownloadId,
        properties: DownloadProperties,
    ) -> Result<()> {
        properties.validate()?;
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        let index = data
            .downloads
            .iter()
            .position(|d| d.id == id)
            .ok_or(Error::NotFound)?;
        let mut download = data.downloads[index].clone();
        download.properties = properties;
        self.inner.store.save(&download, index).await?;
        data.downloads[index] = download;
        Ok(())
    }

    pub async fn set_credentials(
        &self,
        id: DownloadId,
        credentials: Option<BasicCredentials>,
    ) -> Result<()> {
        let data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        let download = data
            .downloads
            .iter()
            .find(|d| d.id == id)
            .ok_or(Error::NotFound)?;
        if data.running.contains_key(&id)
            || !matches!(
                download.state,
                DownloadState::Paused | DownloadState::Failed
            )
        {
            return Err(Error::Invalid("pause before changing credentials".into()));
        }
        let url = validated_url(&download.url)?;
        validate_credentials(&url, credentials.as_ref())?;
        let mut secrets = self.inner.credentials.lock().await;
        if let Some(credentials) = credentials {
            secrets.insert(id, (url.origin().ascii_serialization(), credentials));
        } else {
            secrets.remove(&id);
        }
        Ok(())
    }

    /// Retain partial bytes for a rename in the same directory. Directory changes
    /// are allowed before allocation, without moving or truncating partial data.
    pub async fn update_destination(
        &self,
        id: DownloadId,
        destination: std::path::PathBuf,
    ) -> Result<()> {
        if !destination.is_absolute() || destination.file_name().is_none() {
            return Err(Error::Invalid(
                "destination must be an absolute filename".into(),
            ));
        }
        let parent = tokio::fs::canonicalize(destination.parent().unwrap()).await?;
        let destination = parent.join(destination.file_name().unwrap());
        let mut data = self.inner.data.lock().await;
        if data.stopped {
            return Err(Error::Shutdown);
        }
        let index = data
            .downloads
            .iter()
            .position(|d| d.id == id)
            .ok_or(Error::NotFound)?;
        let mut download = data.downloads[index].clone();
        if data.running.contains_key(&id)
            || !matches!(
                download.state,
                DownloadState::Paused | DownloadState::Failed | DownloadState::Canceled
            )
        {
            return Err(Error::Invalid(
                "pause an unpublished download before changing its destination".into(),
            ));
        }
        if download.ready_to_publish {
            return Err(Error::Invalid(
                "resume publication recovery before editing the destination".into(),
            ));
        }
        if destination == download.destination {
            return Ok(());
        }
        if tokio::fs::try_exists(&destination).await?
            || data
                .downloads
                .iter()
                .any(|d| d.id != id && same_destination(&d.destination, &destination))
        {
            return Err(Error::Invalid(
                "destination already exists or is reserved".into(),
            ));
        }
        if download.destination.parent() != Some(parent.as_path()) {
            if !download.segments.is_empty()
                || tokio::fs::try_exists(&download.temporary_path).await?
            {
                return Err(Error::Invalid("partial data exists; keep its folder and edit only the filename, or cancel and add a new download".into()));
            }
            if let Some(directory) = download.temporary_directory.clone() {
                storage::validate_directory(directory, &parent).await?;
            } else {
                download.temporary_path = parent.join(format!(".osdm-{id}.part"));
            }
        }
        download.destination = destination;
        self.inner.store.save(&download, index).await?;
        data.downloads[index] = download;
        Ok(())
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
        self.inner.credentials.lock().await.remove(&id);
        self.inner.captured_headers.lock().await.remove(&id);
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
    pub async fn shutdown_if_idle(&self) -> Result<bool> {
        {
            let mut data = self.inner.data.lock().await;
            if !data.running.is_empty()
                || !data.synchronizing.is_empty()
                || !data.automation.pending_captures.is_empty()
                || data.downloads.iter().any(|d| {
                    matches!(
                        d.state,
                        DownloadState::Queued
                            | DownloadState::Downloading
                            | DownloadState::Pausing
                            | DownloadState::Canceling
                    )
                })
            {
                return Ok(false);
            }
            data.stopped = true;
        }
        self.shutdown().await?;
        Ok(true)
    }

    /// Pauses all work and waits for workers to drain. This engine cannot be restarted.
    pub async fn shutdown(&self) -> Result<()> {
        let synchronizing: Vec<_> = {
            let mut data = self.inner.data.lock().await;
            data.stopped = true;
            for token in data.synchronizing.values() {
                token.cancel();
            }
            data.synchronizing.keys().copied().collect()
        };
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
        for id in synchronizing {
            self.wait_sync_idle(id).await;
        }
        let data = self.inner.data.lock().await;
        let mut ownership = self.inner.database_lock.lock().await;
        if ownership.is_none() {
            return Ok(());
        }
        for (position, download) in data.downloads.iter().enumerate() {
            self.inner.store.save(download, position).await?;
        }
        self.inner.credentials.lock().await.clear();
        self.inner.captured_headers.lock().await.clear();
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
    if data.downloads.iter().any(|download| {
        download.id == id
            && matches!(
                download.state,
                DownloadState::Completed | DownloadState::Canceled
            )
    }) {
        inner.credentials.lock().await.remove(&id);
        inner.captured_headers.lock().await.remove(&id);
    }
    inner.download_hosts.lock().await.remove(&id);
}
