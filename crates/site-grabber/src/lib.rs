//! Network-free, resumable, same-origin site discovery. See the crate README.
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use url::Url;

const MAX_URL: usize = 8192;
const MAX_LINKS: usize = 2048;
const MAX_BODY: u64 = 512 * 1024;
const MAX_ROBOTS: u64 = 64 * 1024;
// At most six persisted copies per URL, each at most 6x JSON escaping: 36 MiB.
// Bounded config (<7 MiB), robots (<1 MiB), and 20k records/intents leave
// substantial headroom under the unchanged 64 MiB restore limit.
const MAX_STATE_URL_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Limits {
    pub pages: u32,
    pub files: u32,
    pub bytes: u64,
    pub depth: u8,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            pages: 100,
            files: 100,
            bytes: 10 * 1024 * 1024,
            depth: 4,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub seed: String,
    /// Absolute URL path, ending in `/`. Must contain the seed.
    pub path_prefix: String,
    /// Lowercase ASCII extensions without dots; only these files are offered.
    pub extensions: BTreeSet<String>,
    /// Absolute path prefixes to exclude, including their descendants.
    pub excluded_paths: Vec<String>,
    pub limits: Limits,
}

pub type Budgets = Limits;

#[derive(Clone, Serialize, Deserialize)]
pub struct Scope {
    pub path_prefix: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Filters {
    pub extensions: BTreeSet<String>,
    pub excluded_paths: Vec<String>,
}

/// Stable wizard configuration. Source URLs and snapshots are secrets.
#[derive(Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub source: String,
    pub scope: Scope,
    pub filters: Filters,
    pub budgets: Budgets,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub config: ProjectConfig,
    pub confirmed: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProjectSnapshot {
    pub version: u8,
    pub config: ProjectConfig,
    pub confirmed: bool,
    pub paused: bool,
    pub stopped: bool,
    pub robots_ready: bool,
    pub fetched_bytes: u64,
    pub reserved_bytes: u64,
    pub pending_fetches: u32,
    pub in_flight: Option<FetchIntent>,
    pub resources: Vec<Resource>,
}

impl From<ProjectConfig> for Config {
    fn from(config: ProjectConfig) -> Self {
        Self {
            seed: config.source,
            path_prefix: config.scope.path_prefix,
            extensions: config.filters.extensions,
            excluded_paths: config.filters.excluded_paths,
            limits: config.budgets,
        }
    }
}

impl From<Config> for ProjectConfig {
    fn from(config: Config) -> Self {
        Self {
            source: config.seed,
            scope: Scope {
                path_prefix: config.path_prefix,
            },
            filters: Filters {
                extensions: config.extensions,
                excluded_paths: config.excluded_paths,
            },
            budgets: config.limits,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FetchKind {
    Robots,
    Page,
    /// HEAD only, never a file-body GET.
    ResourceMetadata,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FetchIntent {
    pub url: String,
    pub depth: u8,
    pub kind: FetchKind,
    /// Caller MUST enforce this cap during streaming, not after buffering.
    pub max_bytes: u64,
}
impl std::fmt::Debug for FetchIntent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetchIntent")
            .field("url", &"[redacted]")
            .field("depth", &self.depth)
            .field("max_bytes", &self.max_bytes)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
pub struct FetchedResponse {
    pub status: u16,
    /// Must equal the intent URL. Engine automatic redirects MUST be disabled.
    pub final_url: String,
    pub content_type: String,
    pub body: Vec<u8>,
    /// All received body bytes, including any discarded due to a streaming cap.
    pub received_bytes: u64,
    pub location: Option<String>,
    /// Engine-parsed identity-encoding HEAD Content-Length; never inferred as zero.
    pub content_length: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Resource {
    pub url: String,
    /// Flat, generated ASCII name: never derived from an untrusted path.
    pub filename: String,
    pub known_bytes: Option<u64>,
    pub metadata_requested: bool,
    pub selected_bytes: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
struct State {
    version: u8,
    config: Config,
    confirmed: bool,
    paused: bool,
    stopped: bool,
    robots_ready: bool,
    disallow: Vec<String>,
    frontier: VecDeque<FetchIntent>,
    in_flight: Option<FetchIntent>,
    seen: BTreeSet<String>,
    #[serde(default)]
    page_urls: BTreeSet<String>,
    #[serde(skip)]
    persistent_url_bytes: usize,
    resources: BTreeMap<String, Resource>,
    pages: u32,
    fetched_bytes: u64,
    reserved_bytes: u64,
}

#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct Project(State);

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidConfig,
    InvalidState,
    NotConfirmed,
    Paused,
    NoFetch,
    InvalidResponse,
    Budget,
    UnknownResource,
    UnknownLength,
    OutOfScope,
    MetadataMismatch,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "site grabber: {self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

fn canonical(raw: &str) -> Option<Url> {
    if raw.len() > MAX_URL {
        return None;
    }
    let mut url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.set_fragment(None);
    if url.as_str().len() > MAX_URL {
        return None;
    }
    Some(url)
}

fn extension(url: &Url) -> Option<String> {
    url.path()
        .rsplit('/')
        .next()
        .unwrap_or("")
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
}

fn crawlable(url: &Url) -> bool {
    extension(url).is_none_or(|e| matches!(e.as_str(), "html" | "htm"))
}

fn safe_path(path: &str) -> bool {
    // Reject encoded separators/dot segments rather than guessing server decoding.
    let lower = path.to_ascii_lowercase();
    !lower.contains('%') && !lower.contains('\\') && !lower.contains("//")
}

fn valid_config(c: &Config) -> bool {
    let Some(seed) = canonical(&c.seed) else {
        return false;
    };
    let l = &c.limits;
    c.path_prefix.starts_with('/')
        && c.path_prefix.ends_with('/')
        && !c.path_prefix.contains(['?', '#'])
        && safe_path(&c.path_prefix)
        && seed.path().starts_with(&c.path_prefix)
        && safe_path(seed.path())
        && l.pages > 0
        && l.pages <= 10_000
        && l.files > 0
        && l.files <= 10_000
        && l.bytes > 0
        && l.bytes <= 10 * 1024 * 1024 * 1024
        && l.depth <= 16
        && !c.extensions.is_empty()
        && c.extensions.len() <= 128
        && c.extensions.iter().all(|e| {
            !e.is_empty()
                && e.len() <= 16
                && e.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
        && c.excluded_paths.len() <= 128
        && c.excluded_paths
            .iter()
            .all(|p| p.starts_with('/') && p.len() <= MAX_URL && safe_path(p))
}

impl Project {
    pub fn create(request: CreateProjectRequest) -> Result<Self> {
        let mut project = Self::new(request.config.into())?;
        if request.confirmed {
            project.confirm();
        }
        Ok(project)
    }

    /// UI DTO only. Persist the Project itself for resumable frontier state.
    pub fn snapshot(&self) -> ProjectSnapshot {
        let s = &self.0;
        ProjectSnapshot {
            version: s.version,
            config: s.config.clone().into(),
            confirmed: s.confirmed,
            paused: s.paused,
            stopped: s.stopped,
            robots_ready: s.robots_ready,
            fetched_bytes: s.fetched_bytes,
            reserved_bytes: s.reserved_bytes,
            pending_fetches: (s.frontier.len() + usize::from(s.in_flight.is_some())) as u32,
            in_flight: s.in_flight.clone(),
            resources: s.resources.values().cloned().collect(),
        }
    }

    pub fn new(mut config: Config) -> Result<Self> {
        if !valid_config(&config) {
            return Err(Error::InvalidConfig);
        }
        config.seed = canonical(&config.seed).unwrap().to_string();
        Ok(Self(State {
            version: 1,
            config,
            confirmed: false,
            paused: false,
            stopped: false,
            robots_ready: false,
            disallow: vec![],
            frontier: VecDeque::new(),
            in_flight: None,
            seen: BTreeSet::new(),
            page_urls: BTreeSet::new(),
            persistent_url_bytes: 0,
            resources: BTreeMap::new(),
            pages: 0,
            fetched_bytes: 0,
            reserved_bytes: 0,
        }))
    }

    /// The only supported deserialization entry point. Treat snapshots as secrets.
    pub fn restore(json: &str) -> Result<Self> {
        if json.len() > 64 * 1024 * 1024 {
            return Err(Error::InvalidState);
        }
        let mut state: State = serde_json::from_str(json).map_err(|_| Error::InvalidState)?;
        // Legacy v1 states used disjoint pages/resources. Recover only their
        // unambiguous page identities; validation below rejects forged binaries.
        if state.page_urls.is_empty() && state.pages != 0 {
            state.page_urls = state
                .seen
                .iter()
                .filter(|u| !state.resources.contains_key(*u))
                .cloned()
                .collect();
        }
        state.persistent_url_bytes = state.seen.iter().map(String::len).sum();
        let project = Self(state);
        if !project.valid_state() {
            return Err(Error::InvalidState);
        }
        Ok(project)
    }

    pub fn confirm(&mut self) {
        if self.0.confirmed {
            return;
        }
        self.0.confirmed = true;
        let mut robots = canonical(&self.0.config.seed).unwrap();
        robots.set_path("/robots.txt");
        robots.set_query(None);
        self.0.frontier.push_back(FetchIntent {
            url: robots.to_string(),
            depth: 0,
            kind: FetchKind::Robots,
            max_bytes: 0,
        });
    }
    pub fn pause(&mut self) {
        self.0.paused = true;
    }
    pub fn resume(&mut self) {
        self.0.paused = false;
    }
    pub fn resources(&self) -> impl Iterator<Item = &Resource> {
        self.0.resources.values()
    }
    pub fn fetched_bytes(&self) -> u64 {
        self.0.fetched_bytes
    }
    pub fn reserved_bytes(&self) -> u64 {
        self.0.reserved_bytes
    }
    pub fn is_stopped(&self) -> bool {
        self.0.stopped
    }

    /// Validate a selected-resource request/redirect before engine transport.
    /// The engine must additionally enforce the selected resource's byte reservation.
    pub fn validate_target(&self, target: &str) -> Result<String> {
        if !self.0.confirmed {
            return Err(Error::NotConfirmed);
        }
        let url = canonical(target).ok_or(Error::OutOfScope)?;
        if !self.0.robots_ready || !self.scoped(&url) || !self.allowed(&url) {
            return Err(Error::OutOfScope);
        }
        Ok(url.to_string())
    }

    /// Replays the same in-flight intent until `accept` consumes it.
    pub fn next_fetch(&mut self) -> Result<Option<FetchIntent>> {
        if !self.0.confirmed {
            return Err(Error::NotConfirmed);
        }
        if self.0.paused {
            return Err(Error::Paused);
        }
        if self.0.stopped {
            return Ok(None);
        }
        if self.0.in_flight.is_none() {
            let Some(mut intent) = self.0.frontier.pop_front() else {
                return Ok(None);
            };
            let remaining = self.remaining();
            if remaining == 0 {
                self.0.stopped = true;
                return Ok(None);
            }
            intent.max_bytes = remaining.min(if intent.kind == FetchKind::Robots {
                MAX_ROBOTS
            } else {
                MAX_BODY
            });
            self.0.in_flight = Some(intent);
        }
        Ok(self.0.in_flight.clone())
    }

    /// Accept exactly one manually fetched response. Errors stop exploration fail-closed.
    pub fn accept(&mut self, response: FetchedResponse) -> Result<()> {
        if self.0.paused {
            return Err(Error::Paused);
        }
        let intent = self.0.in_flight.take().ok_or(Error::NoFetch)?;
        self.0.fetched_bytes = self.0.fetched_bytes.saturating_add(response.received_bytes);
        let final_url = canonical(&response.final_url);
        if !(100..=599).contains(&response.status)
            || final_url.as_ref().map(Url::as_str) != Some(intent.url.as_str())
            || response.received_bytes != response.body.len() as u64
            || response.received_bytes > intent.max_bytes
            || self.total() > self.0.config.limits.bytes
        {
            self.0.stopped = true;
            return Err(Error::InvalidResponse);
        }
        if intent.kind == FetchKind::ResourceMetadata {
            // HEAD redirects/errors remain unknown: never follow a resource URL
            // automatically or accidentally turn a probe into a file body GET.
            if response.status == 200
                && response.body.is_empty()
                && let Some(resource) = self.0.resources.get_mut(&intent.url)
            {
                resource.known_bytes = response.content_length;
            }
            return Ok(());
        }
        if (300..400).contains(&response.status) {
            // Robots redirects fail closed; page redirects are individually validated.
            if intent.kind == FetchKind::Robots {
                self.0.stopped = true;
                return Ok(());
            }
            if let Some(target) = response
                .location
                .and_then(|s| final_url.unwrap().join(&s).ok())
            {
                self.enqueue(target.as_str(), intent.depth)?;
            }
            return Ok(());
        }
        if intent.kind == FetchKind::Robots {
            match response.status {
                404 | 410 => {}
                200 => {
                    if std::str::from_utf8(&response.body).is_err() {
                        self.0.stopped = true;
                        return Ok(());
                    }
                    self.0.disallow = parse_robots(&response.body);
                }
                _ => {
                    self.0.stopped = true;
                    return Ok(());
                }
            }
            self.0.robots_ready = true;
            self.enqueue(&self.0.config.seed.clone(), 0)?;
            return Ok(());
        }
        if response.status != 200
            || !response
                .content_type
                .split(';')
                .next()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/html"))
        {
            return Ok(());
        }
        let html = Html::parse_document(&String::from_utf8_lossy(&response.body));
        let selector = Selector::parse(
            "a[href], area[href], img[src], source[src], video[src], audio[src], link[href]",
        )
        .unwrap();
        let base = final_url.unwrap();
        for node in html.select(&selector).take(MAX_LINKS) {
            if let Some(raw) = node
                .value()
                .attr("href")
                .or_else(|| node.value().attr("src"))
                && raw.len() <= MAX_URL
                && let Ok(url) = base.join(raw)
            {
                self.enqueue(url.as_str(), intent.depth.saturating_add(1))?;
            }
        }
        Ok(())
    }

    /// Schedule exactly one engine-owned HEAD for a discovered resource.
    /// Unknown lengths are retained explicitly and cannot be selected.
    pub fn request_metadata(&mut self, url: &str) -> Result<()> {
        let key = self.validate_target(url)?;
        if self.0.stopped {
            return Err(Error::Budget);
        }
        let resource = self
            .0
            .resources
            .get_mut(&key)
            .ok_or(Error::UnknownResource)?;
        if resource.metadata_requested {
            return Ok(());
        }
        resource.metadata_requested = true;
        self.0.frontier.push_front(FetchIntent {
            url: key,
            depth: 0,
            kind: FetchKind::ResourceMetadata,
            max_bytes: 0,
        });
        Ok(())
    }

    /// Review-only admission; supplied size must match successful HEAD metadata.
    /// Does not download. Unknown sizes cannot be selected. Reservations are final.
    pub fn select(&mut self, url: &str, bytes: Option<u64>) -> Result<Resource> {
        if !self.0.confirmed {
            return Err(Error::NotConfirmed);
        }
        let key = canonical(url).ok_or(Error::UnknownResource)?.to_string();
        let resource = self.0.resources.get(&key).ok_or(Error::UnknownResource)?;
        if resource.selected_bytes.is_some() {
            return Ok(resource.clone());
        }
        let size = bytes.ok_or(Error::UnknownLength)?;
        let known = resource.known_bytes.ok_or(Error::UnknownLength)?;
        if size != known {
            return Err(Error::MetadataMismatch);
        }
        // Preserve any already issued request's full allowance.
        let pending = self.0.in_flight.as_ref().map_or(0, |i| i.max_bytes);
        if size > self.remaining().saturating_sub(pending) {
            return Err(Error::Budget);
        }
        self.0.reserved_bytes += size;
        let resource = self.0.resources.get_mut(&key).unwrap();
        resource.selected_bytes = Some(size);
        Ok(resource.clone())
    }

    fn total(&self) -> u64 {
        self.0.fetched_bytes.saturating_add(self.0.reserved_bytes)
    }
    fn remaining(&self) -> u64 {
        self.0.config.limits.bytes.saturating_sub(self.total())
    }
    fn scoped(&self, url: &Url) -> bool {
        let seed = canonical(&self.0.config.seed).unwrap();
        url.origin() == seed.origin()
            && safe_path(url.path())
            && url.path().starts_with(&self.0.config.path_prefix)
            && !self
                .0
                .config
                .excluded_paths
                .iter()
                .any(|p| url.path().starts_with(p))
    }
    fn allowed(&self, url: &Url) -> bool {
        let target = &url[url::Position::BeforePath..url::Position::AfterQuery];
        !self
            .0
            .disallow
            .iter()
            .any(|rule| robots_match(rule, target))
    }
    fn enqueue(&mut self, raw: &str, depth: u8) -> Result<()> {
        let Some(url) = canonical(raw) else {
            return Ok(());
        };
        if depth > self.0.config.limits.depth
            || !self.scoped(&url)
            || !self.allowed(&url)
            || self.0.seen.contains(url.as_str())
        {
            return Ok(());
        }
        let ext = extension(&url);
        let page = crawlable(&url) && self.0.pages < self.0.config.limits.pages;
        let resource_ext = ext.as_ref().filter(|e| {
            self.0.config.extensions.contains(*e)
                && self.0.resources.len() < self.0.config.limits.files as usize
        });
        if !page && resource_ext.is_none() {
            return Ok(());
        }
        if self.0.persistent_url_bytes + url.as_str().len() > MAX_STATE_URL_BYTES {
            self.0.stopped = true;
            return Err(Error::Budget);
        }
        let key = url.to_string();
        self.0.persistent_url_bytes += key.len();
        self.0.seen.insert(key.clone());
        if let Some(ext) = resource_ext {
            self.0.resources.insert(
                key.clone(),
                Resource {
                    url: key.clone(),
                    filename: format!("resource-{:05}.{ext}", self.0.resources.len() + 1),
                    known_bytes: None,
                    metadata_requested: false,
                    selected_bytes: None,
                },
            );
        }
        if page {
            self.0.pages += 1;
            self.0.page_urls.insert(key.clone());
            self.0.frontier.push_back(FetchIntent {
                url: key,
                depth,
                kind: FetchKind::Page,
                max_bytes: 0,
            });
        }
        Ok(())
    }

    fn valid_state(&self) -> bool {
        let s = &self.0;
        if s.version != 1
            || !valid_config(&s.config)
            || s.pages > s.config.limits.pages
            || s.resources.len() > s.config.limits.files as usize
            || (self.total() > s.config.limits.bytes && !s.stopped)
            || s.page_urls.len() != s.pages as usize
            || s.persistent_url_bytes > MAX_STATE_URL_BYTES
            || s.seen
                .iter()
                .any(|u| !s.page_urls.contains(u) && !s.resources.contains_key(u))
            || s.page_urls
                .iter()
                .any(|u| !s.seen.contains(u) || !canonical(u).is_some_and(|u| crawlable(&u)))
            || s.frontier.len() > s.pages as usize + s.resources.len() + 1
            || s.disallow.len() > MAX_ROBOTS as usize
            || s.disallow.iter().map(String::len).sum::<usize>() > MAX_ROBOTS as usize
            || (!s.confirmed
                && (s.robots_ready
                    || s.pages != 0
                    || !s.frontier.is_empty()
                    || s.in_flight.is_some()))
        {
            return false;
        }
        let seed = canonical(&s.config.seed).unwrap();
        let mut robots = seed.clone();
        robots.set_path("/robots.txt");
        robots.set_query(None);
        let mut pending = BTreeSet::new();
        for i in s.frontier.iter().chain(s.in_flight.iter()) {
            let kind = match i.kind {
                FetchKind::Robots => 0,
                FetchKind::Page => 1,
                FetchKind::ResourceMetadata => 2,
            };
            if !pending.insert((&i.url, kind))
                || i.depth > s.config.limits.depth
                || i.max_bytes > MAX_BODY
                || i.max_bytes > self.remaining()
            {
                return false;
            }
            if i.kind == FetchKind::Robots {
                if s.robots_ready || i.url != robots.as_str() || i.max_bytes > MAX_ROBOTS {
                    return false;
                }
            } else if !s.robots_ready
                || !s.seen.contains(&i.url)
                || !canonical(&i.url).is_some_and(|u| self.scoped(&u) && self.allowed(&u))
            {
                return false;
            }
            if i.kind == FetchKind::ResourceMetadata
                && !s
                    .resources
                    .get(&i.url)
                    .is_some_and(|r| r.metadata_requested && r.known_bytes.is_none())
            {
                return false;
            }
            if i.kind == FetchKind::Page && !s.page_urls.contains(&i.url) {
                return false;
            }
        }
        if !s.seen.iter().all(|key| {
            canonical(key).is_some_and(|u| u.as_str() == key && self.scoped(&u) && self.allowed(&u))
        }) {
            return false;
        }
        let mut names = BTreeSet::new();
        let mut reserved = 0u64;
        for (key, r) in &s.resources {
            let Some(url) = canonical(key) else {
                return false;
            };
            let ext = url
                .path()
                .rsplit_once('.')
                .map(|(_, e)| e.to_ascii_lowercase())
                .unwrap_or_default();
            if key != &r.url
                || !s.seen.contains(key)
                || !s.config.extensions.contains(&ext)
                || (r.known_bytes.is_some() && !r.metadata_requested)
                || (r.selected_bytes.is_some() && r.selected_bytes != r.known_bytes)
                || !names.insert(&r.filename)
                || !r
                    .filename
                    .strip_prefix("resource-")
                    .and_then(|name| name.strip_suffix(&format!(".{ext}")))
                    .and_then(|n| n.parse::<usize>().ok())
                    .is_some_and(|n| {
                        n > 0
                            && n <= s.resources.len()
                            && r.filename == format!("resource-{n:05}.{ext}")
                    })
            {
                return false;
            }
            let Some(sum) = reserved.checked_add(r.selected_bytes.unwrap_or(0)) else {
                return false;
            };
            reserved = sum;
        }
        reserved == s.reserved_bytes && s.disallow.iter().all(|r| r.len() <= MAX_ROBOTS as usize)
    }
}

fn parse_robots(body: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(body);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    // Only a single leading BOM is a document marker. Elsewhere it could hide
    // an agent/directive name; deny rather than silently ignoring that group.
    if text.contains('\u{feff}') {
        return vec!["/".to_owned()];
    }
    let mut applies = false;
    let mut directives = false;
    let mut rules = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if key.trim().eq_ignore_ascii_case("user-agent") {
            if directives {
                applies = false;
                directives = false;
            }
            applies |= value == "*" || value.to_ascii_lowercase().contains("osdm");
        } else {
            directives = true;
            if applies && key.trim().eq_ignore_ascii_case("disallow") && !value.is_empty() {
                // Malformed non-path rules deny everything rather than bypassing policy.
                rules.push(if value.starts_with('/') && !value.contains('%') {
                    value.to_owned()
                } else {
                    "/".to_owned()
                });
            }
        }
    }
    rules
}

fn robots_match(rule: &str, target: &str) -> bool {
    // Conservative wildcard support: `$` is ignored (may deny extra URLs).
    let rule = rule.strip_suffix('$').unwrap_or(rule);
    let mut remaining = target;
    for (index, part) in rule.split('*').enumerate() {
        if part.is_empty() {
            continue;
        }
        if index == 0 {
            let Some(rest) = remaining.strip_prefix(part) else {
                return false;
            };
            remaining = rest;
        } else if let Some(pos) = remaining.find(part) {
            remaining = &remaining[pos + part.len()..];
        } else {
            return false;
        }
    }
    true
}
