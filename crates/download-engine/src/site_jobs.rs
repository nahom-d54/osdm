//! Persisted, budgeted site exploration and explicitly reviewed resource admission.
//! Existing-file integration requirements: .foreman/scratch/site-integration-hooks.md.
use crate::{DownloadId, DownloadRequest, DownloadState, Engine, Error, Result, transfer};
use futures_util::StreamExt;
use reqwest::header;
use serde::{Deserialize, Serialize};
use site_grabber::{FetchIntent, FetchKind, FetchedResponse, Project};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, Notify, OwnedRwLockWriteGuard, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

// URL-bearing DTOs deliberately do not implement Debug.
#[derive(Clone, Serialize, Deserialize)]
pub struct SiteProjectRequest {
    pub name: String,
    pub seed_url: String,
    pub path_prefix: String,
    pub destination: PathBuf,
    pub extensions: Vec<String>,
    pub excluded_paths: Vec<String>,
    pub max_pages: u32,
    pub max_files: u32,
    pub max_bytes: u64,
    pub max_depth: u8,
    pub confirmed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteProjectState {
    Paused,
    Exploring,
    Ready,
    Failed,
    Canceled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SiteAction {
    Pause,
    Resume,
    Cancel,
    Delete,
    Recheck,
}

/// Opt-in unattended policy. Finite project budgets also apply across rechecks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SitePolicy {
    pub automatic_downloads: bool,
    pub max_concurrent_downloads: usize,
    pub cache_responses: bool,
    pub next_run: Option<u64>,
    pub interval_seconds: Option<u64>,
    pub catch_up_on_startup: bool,
}

impl Default for SitePolicy {
    fn default() -> Self {
        Self {
            automatic_downloads: false,
            max_concurrent_downloads: 1,
            cache_responses: false,
            next_run: None,
            interval_seconds: None,
            catch_up_on_startup: false,
        }
    }
}

impl SitePolicy {
    fn validate(&self) -> Result<()> {
        if !(1..=32).contains(&self.max_concurrent_downloads)
            || self.next_run.is_some_and(|time| time > 253_402_300_799)
            || self
                .interval_seconds
                .is_some_and(|seconds| !(60..=31_536_000).contains(&seconds))
            || (self.interval_seconds.is_some() && self.next_run.is_none())
        {
            return Err(invalid("invalid site concurrency or UTC schedule"));
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SiteResource {
    pub id: String,
    pub url: String,
    pub relative_path: String,
    pub size: Option<u64>,
    pub download_id: Option<DownloadId>,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SiteProject {
    pub id: Uuid,
    pub name: String,
    pub destination: PathBuf,
    pub state: SiteProjectState,
    pub pages: u32,
    /// Conservative discovery accounting + selected payload reservations.
    /// Not ordinary download progress; selected retransmission traffic is excluded.
    pub bytes: u64,
    pub resources: Vec<SiteResource>,
    pub error: Option<String>,
    #[serde(default)]
    pub policy: SitePolicy,
    #[serde(default)]
    pub generation: u64,
    #[serde(default)]
    pub last_run: Option<u64>,
    #[serde(default)]
    pub schedule_note: Option<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub excluded_paths: Vec<String>,
}

// Secret-bearing local conditional cache; never Debug/logged. Always revalidated,
// never used for robots, resource bodies, credentials, or stale-on-error fallback.
#[derive(Clone, Serialize, Deserialize)]
struct CachedPage {
    url: String,
    etag: String,
    content_type: String,
    body: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SavedSiteProject {
    pub view: SiteProject,
    core: String,
    max_bytes: u64,
    /// Pessimistic charges for attempts not durably accepted by the core.
    extra_bytes: u64,
    /// Escrow persisted before a request; charged in full on interrupted recovery.
    active_allowance: u64,
    #[serde(default)]
    history: Vec<SiteResource>,
    #[serde(default)]
    cache: Vec<CachedPage>,
}

struct State {
    jobs: Vec<SavedSiteProject>,
    running: HashMap<Uuid, CancellationToken>,
    drained: bool,
}

pub(crate) struct SiteRuntime {
    state: Mutex<State>,
    pub operations: Arc<RwLock<()>>,
    pub shutdown: CancellationToken,
    changed: Notify,
    client: reqwest::Client,
}

fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn core_error(_: site_grabber::Error) -> Error {
    invalid("site scope, metadata, or budget rejected the operation")
}

fn resource_path(id: Uuid, generation: u64, filename: &str) -> String {
    if generation == 0 {
        filename.to_owned()
    } else {
        format!("site-{id}-r{generation}-{filename}")
    }
}

fn resource_id(generation: u64, filename: &str) -> String {
    if generation == 0 {
        filename.to_owned()
    } else {
        format!("r{generation}-{filename}")
    }
}

impl SavedSiteProject {
    fn project(&self) -> Result<Project> {
        Project::restore(&self.core).map_err(core_error)
    }
    fn remaining(&self, core: &Project) -> u64 {
        self.max_bytes.saturating_sub(
            core.fetched_bytes()
                .saturating_add(core.reserved_bytes())
                .saturating_add(self.extra_bytes)
                .saturating_add(self.active_allowance),
        )
    }
    fn refresh(&mut self, core: &Project) -> Result<()> {
        let saved = serde_json::to_string(core)?;
        // Do not durably create a snapshot that the bounded restore API rejects.
        if saved.len() > 64 * 1024 * 1024 {
            return Err(invalid(
                "site snapshot exceeds the supported persistence limit",
            ));
        }
        self.core = saved;
        self.view.bytes = core
            .fetched_bytes()
            .saturating_add(core.reserved_bytes())
            .saturating_add(self.extra_bytes)
            .saturating_add(self.active_allowance);
        let config = core.snapshot().config;
        self.view.extensions = config.filters.extensions.into_iter().collect();
        self.view.excluded_paths = config.filters.excluded_paths;
        let ids: HashMap<_, _> = self
            .view
            .resources
            .iter()
            .map(|r| (r.id.clone(), r.download_id))
            .collect();
        self.view.resources = core
            .resources()
            .map(|r| SiteResource {
                id: resource_id(self.view.generation, &r.filename),
                url: r.url.clone(),
                relative_path: resource_path(self.view.id, self.view.generation, &r.filename),
                size: r.known_bytes,
                download_id: ids
                    .get(&resource_id(self.view.generation, &r.filename))
                    .copied()
                    .flatten(),
                description: if r.description.is_empty() {
                    reqwest::Url::parse(&r.url)
                        .map(|url| {
                            url.path()
                                .rsplit('/')
                                .next()
                                .unwrap_or("")
                                .chars()
                                .take(160)
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    r.description.clone()
                },
            })
            .collect();
        Ok(())
    }

    fn all_children(&self) -> Vec<DownloadId> {
        self.view
            .resources
            .iter()
            .chain(&self.history)
            .filter_map(|resource| resource.download_id)
            .collect()
    }

    /// A new frontier stays in the original scope. Prior files/charges remain;
    /// new content gets a distinct revision path, never an in-place replacement.
    fn reset(&mut self, filters: Option<site_grabber::Filters>) -> Result<()> {
        let previous = self.project()?;
        let mut config = previous.snapshot().config;
        if let Some(filters) = filters {
            config.filters = filters;
        }
        let mut history = self.history.clone();
        let mut known: std::collections::HashSet<_> = history
            .iter()
            .filter_map(|resource| resource.download_id)
            .collect();
        history.extend(
            self.view
                .resources
                .iter()
                .filter(|resource| resource.download_id.is_some_and(|id| known.insert(id)))
                .cloned(),
        );
        if history.len() > 10_000
            || history
                .iter()
                .map(|resource| resource.url.len())
                .sum::<usize>()
                > 1024 * 1024
        {
            return Err(invalid(
                "site retained revisions reached their project limit",
            ));
        }
        let consumed = self.view.bytes;
        if consumed >= self.max_bytes {
            return Err(invalid("site lifetime payload budget exhausted"));
        }
        let mut core = Project::create(site_grabber::CreateProjectRequest {
            config,
            confirmed: true,
        })
        .map_err(core_error)?;
        core.pause();
        self.view.generation = self
            .view
            .generation
            .checked_add(1)
            .filter(|generation| *generation <= 1_000_000)
            .ok_or_else(|| invalid("site revision limit reached"))?;
        self.history = history;
        self.extra_bytes = consumed;
        self.active_allowance = 0;
        self.view.resources.clear();
        self.view.pages = 0;
        self.view.error = None;
        self.view.state = SiteProjectState::Paused;
        self.refresh(&core)
    }
    fn abandon_attempt(&mut self) {
        self.extra_bytes = self.extra_bytes.saturating_add(self.active_allowance);
        self.active_allowance = 0;
    }
}

impl SiteRuntime {
    pub(crate) async fn concurrency_limits(
        &self,
    ) -> Vec<(std::collections::HashSet<DownloadId>, usize)> {
        self.state
            .lock()
            .await
            .jobs
            .iter()
            .map(|job| {
                (
                    job.all_children().into_iter().collect(),
                    job.view.policy.max_concurrent_downloads,
                )
            })
            .collect()
    }
    pub(crate) async fn load(store: &crate::store::Store) -> Result<Self> {
        let mut jobs = store.load_site_projects().await?;
        let mut ids = std::collections::HashSet::new();
        for job in &mut jobs {
            job.view.policy.validate()?;
            if !ids.insert(job.view.id)
                || !job.view.destination.is_absolute()
                || job.view.name.is_empty()
                || job.view.name.len() > 256
                || job.view.generation > 1_000_000
                || job.history.len() > 10_000
                || job
                    .history
                    .iter()
                    .map(|resource| resource.url.len())
                    .sum::<usize>()
                    > 1024 * 1024
                || job.cache.len() > 16
                || job.cache.iter().map(|page| page.body.len()).sum::<usize>() > 512 * 1024
            {
                return Err(invalid("invalid saved site project"));
            }
            let mut core = job.project()?;
            let snapshot = core.snapshot();
            let seed = reqwest::Url::parse(&snapshot.config.source)
                .map_err(|_| invalid("invalid saved site origin"))?;
            for resource in &job.history {
                let safe = crate::media_capture::MediaUrl::parse(&resource.url)?;
                let url = reqwest::Url::parse(safe.as_str())
                    .map_err(|_| invalid("invalid site history URL"))?;
                if resource.download_id.is_none()
                    || url.origin() != seed.origin()
                    || resource.description.len() > 1024
                    || resource.description.chars().any(char::is_control)
                    || !url.path().starts_with(&snapshot.config.scope.path_prefix)
                    || resource.relative_path.len() > 240
                    || !resource.relative_path.starts_with("resource-")
                        && !resource
                            .relative_path
                            .starts_with(&format!("site-{}-r", job.view.id))
                    || !resource
                        .relative_path
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b".-_".contains(&byte))
                {
                    return Err(invalid("invalid saved site revision"));
                }
            }
            for page in &job.cache {
                let safe = crate::media_capture::MediaUrl::parse(&page.url)?;
                let url = reqwest::Url::parse(safe.as_str())
                    .map_err(|_| invalid("invalid cached site URL"))?;
                if url.origin() != seed.origin()
                    || !url.path().starts_with(&snapshot.config.scope.path_prefix)
                    || page.etag.len() > 1024
                    || reqwest::header::HeaderValue::from_str(&page.etag).is_err()
                {
                    return Err(invalid("invalid saved site cache"));
                }
            }
            if !snapshot.confirmed
                || snapshot.config.budgets.bytes != job.max_bytes
                || job.view.pages > snapshot.config.budgets.pages
                || job.active_allowance > 512 * 1024
                || (job.active_allowance > 0
                    && !snapshot
                        .in_flight
                        .as_ref()
                        .is_some_and(|i| job.active_allowance <= i.max_bytes))
                || job.view.resources.len() != snapshot.resources.len()
            {
                return Err(invalid("invalid saved site budget"));
            }
            let mut resource_ids = std::collections::HashSet::new();
            for resource in &job.view.resources {
                if !resource_ids.insert(&resource.id) {
                    return Err(invalid("duplicate saved site resource"));
                }
                let Some(original) = core
                    .resources()
                    .find(|r| resource_id(job.view.generation, &r.filename) == resource.id)
                else {
                    return Err(invalid("invalid saved site resource"));
                };
                if original.url != resource.url
                    || resource.description.len() > 1024
                    || resource.description.chars().any(char::is_control)
                    || resource_path(job.view.id, job.view.generation, &original.filename)
                        != resource.relative_path
                    || original.known_bytes != resource.size
                    || (resource.download_id.is_some() && original.selected_bytes.is_none())
                {
                    return Err(invalid("invalid saved site selection"));
                }
            }
            job.abandon_attempt();
            core.pause();
            if job.view.state == SiteProjectState::Exploring {
                job.view.state = SiteProjectState::Paused;
            }
            job.refresh(&core)?;
            if job.view.bytes > job.max_bytes {
                job.view.state = SiteProjectState::Failed;
                job.view.error = Some("site budget exhausted during interrupted request".into());
            }
            store.save_site_project(job).await?;
        }
        Ok(Self {
            state: Mutex::new(State {
                jobs,
                running: HashMap::new(),
                drained: false,
            }),
            operations: Arc::new(RwLock::new(())),
            shutdown: CancellationToken::new(),
            changed: Notify::new(),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(30))
                .user_agent("OSDM/0.1")
                .build()?,
        })
    }

    /// Caller keeps this guard through setting engine Data.stopped.
    pub(crate) async fn idle_guard(&self) -> Option<OwnedRwLockWriteGuard<()>> {
        let gate = self.operations.clone().try_write_owned().ok()?;
        if !self.state.lock().await.running.is_empty() {
            return None;
        }
        Some(gate)
    }

    async fn wait(&self, id: Uuid) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.state.lock().await.running.contains_key(&id) {
                return;
            }
            changed.await;
        }
    }
}

impl Engine {
    /// Enable unattended work only after explicit confirmation. No request is
    /// sent by configuration itself; schedule slots are consumed before dispatch.
    pub async fn configure_site_policy(
        &self,
        id: Uuid,
        policy: SitePolicy,
        confirmed: bool,
    ) -> Result<()> {
        if !confirmed {
            return Err(invalid("confirm the site automation policy"));
        }
        policy.validate()?;
        let _gate = self.inner.site.operations.read().await;
        self.check_site_open().await?;
        let mut state = self.inner.site.state.lock().await;
        let job = state
            .jobs
            .iter_mut()
            .find(|job| job.view.id == id)
            .ok_or(Error::NotFound)?;
        let mut next = job.clone();
        next.view.policy = policy;
        next.view.schedule_note = None;
        if !next.view.policy.cache_responses {
            next.cache.clear();
        }
        self.inner.store.save_site_project(&next).await?;
        *job = next;
        Ok(())
    }

    /// Re-discover inside the same approved origin/path with new filters. Prior
    /// downloads and lifetime charges remain; no source or output scope expands.
    pub async fn update_site_filters(
        &self,
        id: Uuid,
        filters: site_grabber::Filters,
        confirmed: bool,
    ) -> Result<()> {
        if !confirmed {
            return Err(invalid("confirm the new site filters"));
        }
        // Reject invalid edits before pausing work or consuming schedule intent.
        {
            let state = self.inner.site.state.lock().await;
            let job = state
                .jobs
                .iter()
                .find(|job| job.view.id == id)
                .ok_or(Error::NotFound)?;
            let mut config = job.project()?.snapshot().config;
            config.filters = filters.clone();
            Project::create(site_grabber::CreateProjectRequest {
                config,
                confirmed: true,
            })
            .map_err(core_error)?;
        }
        let canceled = self
            .list_site_projects()
            .await
            .iter()
            .any(|job| job.id == id && job.state == SiteProjectState::Canceled);
        if !canceled {
            self.control_site_project(id, SiteAction::Pause).await?;
        }
        let _gate = self.inner.site.operations.read().await;
        self.check_site_open().await?;
        let mut state = self.inner.site.state.lock().await;
        let job = state
            .jobs
            .iter_mut()
            .find(|job| job.view.id == id)
            .ok_or(Error::NotFound)?;
        let children = job.all_children();
        if self.snapshot().await.iter().any(|download| {
            children.contains(&download.id)
                && !matches!(
                    download.state,
                    DownloadState::Completed | DownloadState::Canceled | DownloadState::Failed
                )
        }) {
            return Err(invalid(
                "finish or cancel queued files before changing site filters",
            ));
        }
        let mut next = job.clone();
        next.reset(Some(filters))?;
        next.cache.clear();
        next.view.policy.next_run = None;
        next.view.policy.interval_seconds = None;
        next.view.policy.automatic_downloads = false;
        self.inner.store.save_site_project(&next).await?;
        *job = next;
        Ok(())
    }

    /// Deterministic UTC scheduler. Missed runs coalesce, overlaps skip, and a
    /// consumed slot cannot run twice after restart or clock reversal.
    #[doc(hidden)]
    pub async fn site_schedule_tick(&self, now: u64, startup: bool) -> Result<()> {
        if !self.inner.schedules_enabled {
            return Ok(());
        }
        let _gate = self.inner.site.operations.read().await;
        self.check_site_open().await?;
        let mut state = self.inner.site.state.lock().await;
        let downloads = self.snapshot().await;
        let ids: Vec<_> = state
            .jobs
            .iter()
            .filter_map(|job| {
                job.view
                    .policy
                    .next_run
                    .filter(|slot| *slot <= now)
                    .map(|_| job.view.id)
            })
            .collect();
        for id in ids {
            let position = state
                .jobs
                .iter()
                .position(|job| job.view.id == id)
                .ok_or(Error::NotFound)?;
            let mut next = state.jobs[position].clone();
            let slot = next
                .view
                .policy
                .next_run
                .ok_or_else(|| invalid("missing site schedule slot"))?;
            next.view.policy.next_run = next.view.policy.interval_seconds.and_then(|interval| {
                (now.saturating_sub(slot) / interval)
                    .checked_add(1)
                    .and_then(|count| count.checked_mul(interval))
                    .and_then(|offset| slot.checked_add(offset))
                    .filter(|time| *time <= 253_402_300_799)
            });
            if next.view.policy.next_run.is_none() {
                next.view.policy.interval_seconds = None;
            }
            next.view.last_run = Some(slot);
            let children = next.all_children();
            let busy = state.running.contains_key(&id)
                || downloads.iter().any(|download| {
                    children.contains(&download.id)
                        && !matches!(
                            download.state,
                            DownloadState::Completed
                                | DownloadState::Failed
                                | DownloadState::Canceled
                        )
                });
            let skip = busy || startup && !next.view.policy.catch_up_on_startup;
            next.view.schedule_note = Some(
                if busy {
                    "Overlapping run skipped"
                } else if skip {
                    "Missed startup run skipped"
                } else {
                    "Scheduled slot consumed"
                }
                .into(),
            );
            // Persist consumption independently of dispatch/reset failures.
            self.inner.store.save_site_project(&next).await?;
            state.jobs[position] = next.clone();
            if skip {
                continue;
            }
            if let Err(error) = next.reset(None) {
                next.view.state = SiteProjectState::Failed;
                next.view.error = Some(error.to_string());
                next.view.policy.next_run = None;
                next.view.policy.interval_seconds = None;
                self.inner.store.save_site_project(&next).await?;
                state.jobs[position] = next;
                continue;
            }
            let mut core = next.project()?;
            core.resume();
            next.view.state = SiteProjectState::Exploring;
            next.refresh(&core)?;
            self.inner.store.save_site_project(&next).await?;
            state.jobs[position] = next;
            let token = self.inner.site.shutdown.child_token();
            state.running.insert(id, token.clone());
            spawn(self.clone(), id, token);
        }
        Ok(())
    }

    pub async fn create_site_project(&self, request: SiteProjectRequest) -> Result<Uuid> {
        let _gate = self.inner.site.operations.read().await;
        self.check_site_open().await?;
        if !request.confirmed {
            return Err(invalid("explicit site project confirmation is required"));
        }
        if request.name.trim().is_empty()
            || request.name.len() > 256
            || request.name.chars().any(char::is_control)
            || !request.destination.is_absolute()
        {
            return Err(invalid(
                "site name and absolute destination directory are required",
            ));
        }
        let destination = tokio::fs::canonicalize(&request.destination).await?;
        if !tokio::fs::metadata(&destination).await?.is_dir() {
            return Err(invalid("site destination must be an existing directory"));
        }
        let core = Project::create(site_grabber::CreateProjectRequest {
            confirmed: true,
            config: site_grabber::ProjectConfig {
                source: request.seed_url,
                scope: site_grabber::Scope {
                    path_prefix: request.path_prefix,
                },
                filters: site_grabber::Filters {
                    extensions: request.extensions.into_iter().collect(),
                    excluded_paths: request.excluded_paths,
                },
                budgets: site_grabber::Budgets {
                    pages: request.max_pages,
                    files: request.max_files,
                    bytes: request.max_bytes,
                    depth: request.max_depth,
                },
            },
        })
        .map_err(core_error)?;
        let id = Uuid::new_v4();
        let mut job = SavedSiteProject {
            view: SiteProject {
                id,
                name: request.name.trim().into(),
                destination,
                state: SiteProjectState::Exploring,
                pages: 0,
                bytes: 0,
                resources: vec![],
                error: None,
                policy: SitePolicy::default(),
                generation: 0,
                last_run: None,
                schedule_note: None,
                extensions: Vec::new(),
                excluded_paths: Vec::new(),
            },
            core: String::new(),
            max_bytes: request.max_bytes,
            extra_bytes: 0,
            active_allowance: 0,
            history: Vec::new(),
            cache: Vec::new(),
        };
        job.refresh(&core)?;
        let token = self.inner.site.shutdown.child_token();
        {
            let mut state = self.inner.site.state.lock().await;
            self.inner.store.save_site_project(&job).await?;
            state.jobs.push(job);
            state.running.insert(id, token.clone());
        }
        spawn(self.clone(), id, token);
        Ok(id)
    }

    pub async fn list_site_projects(&self) -> Vec<SiteProject> {
        self.inner
            .site
            .state
            .lock()
            .await
            .jobs
            .iter()
            .map(|j| j.view.clone())
            .collect()
    }

    async fn check_site_open(&self) -> Result<()> {
        if self.inner.site.shutdown.is_cancelled() || self.inner.data.lock().await.stopped {
            return Err(Error::Shutdown);
        }
        Ok(())
    }

    pub async fn control_site_project(&self, id: Uuid, action: SiteAction) -> Result<()> {
        let _gate = self.inner.site.operations.read().await;
        self.check_site_open().await?;
        if action == SiteAction::Recheck {
            let mut state = self.inner.site.state.lock().await;
            if state.running.contains_key(&id) {
                return Err(invalid("pause exploration before rechecking"));
            }
            let position = state
                .jobs
                .iter()
                .position(|job| job.view.id == id)
                .ok_or(Error::NotFound)?;
            let mut next = state.jobs[position].clone();
            let children = next.all_children();
            if self.snapshot().await.iter().any(|download| {
                children.contains(&download.id)
                    && !matches!(
                        download.state,
                        DownloadState::Completed | DownloadState::Failed | DownloadState::Canceled
                    )
            }) {
                return Err(invalid("finish or cancel queued files before rechecking"));
            }
            next.reset(None)?;
            let mut core = next.project()?;
            core.resume();
            next.view.state = SiteProjectState::Exploring;
            next.refresh(&core)?;
            self.inner.store.save_site_project(&next).await?;
            state.jobs[position] = next;
            let token = self.inner.site.shutdown.child_token();
            state.running.insert(id, token.clone());
            spawn(self.clone(), id, token);
            return Ok(());
        }
        if action == SiteAction::Resume {
            let mut state = self.inner.site.state.lock().await;
            let position = state
                .jobs
                .iter()
                .position(|j| j.view.id == id)
                .ok_or(Error::NotFound)?;
            if state.running.contains_key(&id) {
                return if state.jobs[position].view.state == SiteProjectState::Exploring {
                    Ok(())
                } else {
                    Err(Error::Interrupted)
                };
            }
            let mut next = state.jobs[position].clone();
            if next.view.state == SiteProjectState::Canceled {
                return Err(invalid("canceled site projects cannot resume"));
            }
            let children = next
                .view
                .resources
                .iter()
                .filter_map(|r| r.download_id)
                .collect::<Vec<_>>();
            let mut core = next.project()?;
            core.resume();
            if next.remaining(&core) == 0 {
                return Err(invalid("site byte budget exhausted"));
            }
            next.view.state = SiteProjectState::Exploring;
            next.view.error = None;
            next.refresh(&core)?;
            self.inner.store.save_site_project(&next).await?;
            state.jobs[position] = next;
            let token = self.inner.site.shutdown.child_token();
            state.running.insert(id, token.clone());
            spawn(self.clone(), id, token);
            drop(state);
            for download in self.snapshot().await {
                if children.contains(&download.id) && download.state == DownloadState::Paused {
                    self.resume(download.id).await?;
                }
            }
            return Ok(());
        }
        let children = {
            let mut state = self.inner.site.state.lock().await;
            let position = state
                .jobs
                .iter()
                .position(|j| j.view.id == id)
                .ok_or(Error::NotFound)?;
            let mut next = state.jobs[position].clone();
            if next.view.state == SiteProjectState::Canceled && action == SiteAction::Pause {
                return Err(invalid("canceled site projects cannot be paused"));
            }
            next.view.state = if action == SiteAction::Pause {
                SiteProjectState::Paused
            } else {
                SiteProjectState::Canceled
            };
            next.view.policy.next_run = None;
            next.view.policy.interval_seconds = None;
            next.view.schedule_note = Some("Schedule disabled by project control".into());
            // Persist the control intent BEFORE canceling the worker or children.
            self.inner.store.save_site_project(&next).await?;
            let children = next.all_children();
            state.jobs[position] = next;
            if let Some(token) = state.running.get(&id) {
                token.cancel();
            }
            children
        };
        self.inner.site.wait(id).await;
        for child in children {
            let result = if action == SiteAction::Pause {
                self.pause(child).await
            } else {
                self.cancel(child).await
            };
            if let Err(error) = result
                && !matches!(error, Error::NotFound)
            {
                return Err(error);
            }
        }
        if action == SiteAction::Delete {
            let mut state = self.inner.site.state.lock().await;
            self.inner.store.delete_site_project(id).await?;
            state.jobs.retain(|j| j.view.id != id);
        }
        Ok(())
    }

    /// Explicit review only. Requires persisted exact metadata and hard-capped admission.
    pub async fn queue_site_resources(
        &self,
        id: Uuid,
        resources: Vec<String>,
    ) -> Result<Vec<DownloadId>> {
        let _gate = self.inner.site.operations.read().await;
        self.check_site_open().await?;
        if resources.is_empty() || resources.len() > 10_000 {
            return Err(invalid("select a bounded set of site resources"));
        }
        let mut state = self.inner.site.state.lock().await;
        let position = state
            .jobs
            .iter()
            .position(|j| j.view.id == id)
            .ok_or(Error::NotFound)?;
        let mut next = state.jobs[position].clone();
        if next.view.state != SiteProjectState::Ready {
            return Err(invalid("finish exploration before reviewing downloads"));
        }
        let mut core = next.project()?;
        let mut admissions = Vec::new();
        let mut selected = std::collections::HashSet::new();
        for resource_id in resources {
            if !selected.insert(resource_id.clone()) {
                continue;
            }
            let resource = next
                .view
                .resources
                .iter_mut()
                .find(|r| r.id == resource_id)
                .ok_or(Error::NotFound)?;
            core.validate_target(&resource.url).map_err(core_error)?;
            let bytes = resource
                .size
                .ok_or_else(|| invalid("resource length is unknown; it cannot be queued"))?;
            if resource.download_id.is_none() {
                if bytes
                    > next.max_bytes.saturating_sub(
                        core.fetched_bytes()
                            .saturating_add(core.reserved_bytes())
                            .saturating_add(next.extra_bytes),
                    )
                {
                    return Err(invalid("selected resources exceed site byte budget"));
                }
                core.select(&resource.url, Some(bytes))
                    .map_err(core_error)?;
                resource.download_id = Some(Uuid::new_v4());
            }
            admissions.push((
                resource.download_id.unwrap(),
                resource.url.clone(),
                resource.relative_path.clone(),
                bytes,
            ));
        }
        next.refresh(&core)?;
        // Stable child ids + reservations survive failure between site save and admission.
        self.inner.store.save_site_project(&next).await?;
        state.jobs[position] = next.clone();
        let mut ids = Vec::new();
        for (child, url, filename, bytes) in admissions {
            let admitted = self
                .enqueue_site_child(
                    child,
                    DownloadRequest {
                        url,
                        destination: next.view.destination.join(filename),
                        expected_sha256: None,
                    },
                    bytes,
                )
                .await?;
            if admitted != child {
                return Err(invalid("site child admission changed its durable identity"));
            }
            ids.push(child);
        }
        Ok(ids)
    }

    pub(crate) async fn shutdown_site_projects(&self) -> Result<()> {
        self.inner.site.shutdown.cancel();
        let _gate = self.inner.site.operations.write().await;
        {
            let state = self.inner.site.state.lock().await;
            if state.drained {
                return Ok(());
            }
            for token in state.running.values() {
                token.cancel();
            }
        }
        loop {
            let changed = self.inner.site.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.inner.site.state.lock().await.running.is_empty() {
                break;
            }
            changed.await;
        }
        let mut state = self.inner.site.state.lock().await;
        for job in &mut state.jobs {
            if job.view.state == SiteProjectState::Exploring {
                job.view.state = SiteProjectState::Paused;
            }
            let mut core = job.project()?;
            core.pause();
            job.refresh(&core)?;
            self.inner.store.save_site_project(job).await?;
        }
        state.drained = true;
        Ok(())
    }
}

fn spawn(engine: Engine, id: Uuid, token: CancellationToken) {
    tokio::spawn(async move {
        let worker = engine.clone();
        let explore_token = token.clone();
        let result = tokio::spawn(async move { explore(&worker, id, &explore_token).await })
            .await
            .unwrap_or_else(|_| Err(invalid("site worker terminated unexpectedly")));
        let mut state = engine.inner.site.state.lock().await;
        if let Some(job) = state.jobs.iter_mut().find(|j| j.view.id == id) {
            job.abandon_attempt();
            if let Ok(mut core) = job.project() {
                core.pause();
                if job.view.state == SiteProjectState::Exploring {
                    job.view.state = if engine.inner.site.shutdown.is_cancelled() {
                        SiteProjectState::Paused
                    } else if result.is_ok() {
                        SiteProjectState::Ready
                    } else {
                        SiteProjectState::Failed
                    };
                }
                if result.is_err() && job.view.state == SiteProjectState::Failed {
                    // Never persist reqwest error displays: they can contain signed URLs.
                    job.view.error = Some("site exploration failed or exhausted its budget".into());
                }
                let _ = job.refresh(&core);
            }
            if engine.inner.store.save_site_project(job).await.is_err() {
                job.view.state = SiteProjectState::Failed;
                job.view.error =
                    Some("could not persist site state; stop and reopen the engine".into());
            }
        }
        let mut automatic = Vec::new();
        if !token.is_cancelled()
            && !engine.inner.site.shutdown.is_cancelled()
            && let Some(job) = state.jobs.iter().find(|job| job.view.id == id)
            && job.view.state == SiteProjectState::Ready
            && job.view.policy.automatic_downloads
        {
            let mut remaining = job.max_bytes.saturating_sub(job.view.bytes);
            for resource in &job.view.resources {
                if resource.download_id.is_none()
                    && let Some(size) = resource.size
                    && size <= remaining
                {
                    automatic.push(resource.id.clone());
                    remaining -= size;
                }
            }
        }
        drop(state);
        let automatic_failed = if automatic.is_empty() {
            false
        } else {
            tokio::select! {
                biased;
                _ = token.cancelled() => false,
                _ = engine.inner.site.shutdown.cancelled() => false,
                result = engine.queue_site_resources(id, automatic) => result.is_err(),
            }
        };
        if automatic_failed {
            let mut state = engine.inner.site.state.lock().await;
            if let Some(job) = state.jobs.iter_mut().find(|job| job.view.id == id)
                && job.view.state == SiteProjectState::Ready
            {
                job.view.error = Some(
                    "Automatic download admission failed; review resources before retrying".into(),
                );
                let _ = engine.inner.store.save_site_project(job).await;
            }
        }
        let mut state = engine.inner.site.state.lock().await;
        state.running.remove(&id);
        engine.inner.site.changed.notify_waiters();
    });
}

async fn explore(engine: &Engine, id: Uuid, token: &CancellationToken) -> Result<()> {
    let mut attempt = 0;
    loop {
        if token.is_cancelled() {
            return Err(Error::Interrupted);
        }
        let intent = {
            let mut state = engine.inner.site.state.lock().await;
            let job = state
                .jobs
                .iter_mut()
                .find(|j| j.view.id == id)
                .ok_or(Error::NotFound)?;
            if job.view.state != SiteProjectState::Exploring {
                return Err(Error::Interrupted);
            }
            let mut next = job.clone();
            let mut core = next.project()?;
            core.resume();
            // File metadata is bounded engine-owned HEAD, never a body download.
            let metadata_url = core
                .resources()
                .find(|r| !r.metadata_requested)
                .map(|r| r.url.clone());
            if let Some(url) = metadata_url {
                core.request_metadata(&url).map_err(core_error)?;
            }
            let Some(mut intent) = core.next_fetch().map_err(core_error)? else {
                if core.is_stopped() {
                    return Err(invalid("site exploration stopped by policy"));
                }
                return Ok(());
            };
            intent.max_bytes = intent.max_bytes.min(next.remaining(&core));
            if intent.max_bytes == 0 {
                return Err(invalid("site byte budget exhausted"));
            }
            next.active_allowance = intent.max_bytes;
            next.refresh(&core)?;
            engine.inner.store.save_site_project(&next).await?;
            *job = next;
            intent
        };
        let mut hint = crate::retry::RetryHint::default();
        let mut observed = 0;
        let cached = if intent.kind == FetchKind::Page {
            let state = engine.inner.site.state.lock().await;
            state
                .jobs
                .iter()
                .find(|job| job.view.id == id && job.view.policy.cache_responses)
                .and_then(|job| job.cache.iter().find(|page| page.url == intent.url))
                .cloned()
        } else {
            None
        };
        let fetched = fetch_once(
            engine,
            &intent,
            token,
            &mut hint,
            &mut observed,
            cached.as_ref(),
        )
        .await;
        let retry = match &fetched {
            Ok(r) => matches!(r.status, 429 | 500 | 502 | 503 | 504),
            Err(e) => transfer::retryable(e),
        } && attempt < engine.inner.config.max_retries;
        {
            let mut state = engine.inner.site.state.lock().await;
            let job = state
                .jobs
                .iter_mut()
                .find(|j| j.view.id == id)
                .ok_or(Error::NotFound)?;
            let mut next = job.clone();
            let mut core = next.project()?;
            if next.view.state == SiteProjectState::Exploring
                && !token.is_cancelled()
                && !retry
                && let Ok(response) = fetched.as_ref()
            {
                next.active_allowance = 0;
                // Core consumes one response; retries were separately charged above.
                core.accept(FetchedResponse {
                    status: response.status,
                    final_url: intent.url.clone(),
                    content_type: response.content_type.clone(),
                    body: response.body.clone(),
                    received_bytes: response.body.len() as u64,
                    location: response.location.clone(),
                    content_length: response.content_length,
                })
                .map_err(core_error)?;
                if intent.kind == FetchKind::Page {
                    next.view.pages += 1;
                    next.cache.retain(|page| page.url != intent.url);
                    if next.view.policy.cache_responses
                        && response.cacheable
                        && response.body.len() <= 512 * 1024
                        && let Some(etag) = &response.etag
                    {
                        while next.cache.len() >= 16
                            || next.cache.iter().map(|page| page.body.len()).sum::<usize>()
                                + response.body.len()
                                > 512 * 1024
                        {
                            next.cache.remove(0);
                        }
                        next.cache.push(CachedPage {
                            url: intent.url.clone(),
                            etag: etag.clone(),
                            content_type: response.content_type.clone(),
                            body: response.body.clone(),
                        });
                    }
                }
            } else {
                next.active_allowance = next.active_allowance.max(observed);
                next.abandon_attempt();
            }
            next.refresh(&core)?;
            engine.inner.store.save_site_project(&next).await?;
            *job = next;
        }
        if token.is_cancelled() {
            return Err(Error::Interrupted);
        }
        if retry {
            transfer::wait_before_retry(&engine.inner, &intent.url, attempt, hint, token, None)
                .await?;
            attempt += 1;
        } else {
            fetched?;
            attempt = 0;
        }
    }
}

struct Response {
    status: u16,
    content_type: String,
    body: Vec<u8>,
    location: Option<String>,
    content_length: Option<u64>,
    etag: Option<String>,
    cacheable: bool,
}

async fn fetch_once(
    engine: &Engine,
    intent: &FetchIntent,
    token: &CancellationToken,
    hint: &mut crate::retry::RetryHint,
    observed: &mut u64,
    cached: Option<&CachedPage>,
) -> Result<Response> {
    let operation = async {
        let (lease, _global) =
            transfer::permits(&engine.inner, &intent.url, token, None, false).await?;
        let mut request = engine
            .inner
            .site
            .client
            .request(
                if intent.kind == FetchKind::ResourceMetadata {
                    reqwest::Method::HEAD
                } else {
                    reqwest::Method::GET
                },
                &intent.url,
            )
            .header(header::ACCEPT_ENCODING, "identity");
        if let Some(page) = cached {
            request = request.header(header::IF_NONE_MATCH, &page.etag);
        }
        let response = request.send().await?;
        *hint = transfer::retry_hint(&response);
        if hint.rate_limited {
            let host = response
                .url()
                .host_str()
                .ok_or_else(|| invalid("invalid site host"))?;
            engine
                .inner
                .cooldowns
                .defer(host, crate::retry::delay(0, hint.after))
                .await;
        }
        if response
            .headers()
            .get_all(header::CONTENT_ENCODING)
            .iter()
            .any(|v| v != "identity")
        {
            return Err(invalid("encoded site responses are unsupported"));
        }
        let status = response.status().as_u16();
        let etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() <= 1024)
            .map(str::to_owned);
        let cacheable = intent.kind == FetchKind::Page
            && matches!(status, 200 | 304)
            && response
                .headers()
                .get_all(header::CACHE_CONTROL)
                .iter()
                .all(|value| {
                    value.to_str().is_ok_and(|value| {
                        !value.split(',').any(|directive| {
                            matches!(
                                directive
                                    .trim()
                                    .split('=')
                                    .next()
                                    .unwrap_or("")
                                    .to_ascii_lowercase()
                                    .as_str(),
                                "no-store" | "private"
                            )
                        })
                    })
                })
            && response
                .headers()
                .get_all(header::VARY)
                .iter()
                .all(|value| {
                    value.to_str().is_ok_and(|value| {
                        value
                            .split(',')
                            .all(|name| name.trim().eq_ignore_ascii_case("accept-encoding"))
                    })
                });
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .filter(|v| v.len() <= 8192)
            .map(str::to_owned);
        let lengths = response.headers().get_all(header::CONTENT_LENGTH);
        let content_length = if lengths.iter().count() == 1 {
            lengths
                .iter()
                .next()
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
        } else {
            None
        };
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            *observed = observed.saturating_add(chunk.len() as u64);
            lease.received(chunk.len() as u64);
            if chunk.len() as u64 > intent.max_bytes.saturating_sub(body.len() as u64) {
                return Err(invalid(
                    "site response exceeds its persisted body allowance",
                ));
            }
            transfer::throttle(&engine.inner, chunk.len() as u64, token).await?;
            body.extend_from_slice(&chunk);
        }
        if status == 304 {
            let page =
                cached.ok_or_else(|| invalid("unsolicited site cache validation response"))?;
            if !body.is_empty() || page.body.len() as u64 > intent.max_bytes {
                return Err(invalid(
                    "cached site page exceeds the current body allowance",
                ));
            }
            return Ok(Response {
                status: 200,
                content_type: page.content_type.clone(),
                body: page.body.clone(),
                location: None,
                content_length: Some(page.body.len() as u64),
                etag: etag.or_else(|| Some(page.etag.clone())),
                cacheable,
            });
        }
        let cacheable = cacheable
            && content_type
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/html"));
        Ok(Response {
            status,
            content_type,
            body,
            location,
            content_length,
            etag,
            cacheable,
        })
    };
    tokio::select! {
        biased;
        _ = token.cancelled() => Err(Error::Interrupted),
        _ = engine.inner.site.shutdown.cancelled() => Err(Error::Shutdown),
        result = tokio::time::timeout(Duration::from_secs(60), operation) =>
            result.unwrap_or_else(|_| Err(invalid("site request timed out"))),
    }
}
