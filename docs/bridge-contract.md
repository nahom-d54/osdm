# Desktop bridge and engine contract — available implementation

The native bridge is implemented in `src-tauri/src/lib.rs`. The frontend selects
`TauriBridge` inside Tauri and `MockBridge` in an ordinary browser. The Rust
download engine remains independent of Tauri.

**Status:** the commands, event forwarding, settings store, tray lifecycle, and
engine additions below are available in source. Publisher updates are implemented
but configuration-gated; this repository does not contain a production signing
key or update endpoint. Native desktop release validation is listed in §5.

## 1. Engine data and semantics — available

Serde output is consumed verbatim:

- Struct fields use snake_case (`total_bytes`, `eta_seconds`, `temporary_path`,
  `bytes_per_second`, `ready_to_publish`).
- Optional `expected_sha256`, computed `sha256`, and `integrity` expose checksum
  verification (`Unverified | Verifying | Computed | Verified | Mismatch`).
  `Computed` does not mean verified against a reference.
- Runtime `adaptive` feedback exposes `active_connections`, `host_connections`,
  `host_limit`, `host_ceiling`, `chunk_bytes`, and a human-readable `reason`.
  Durable segments are not a count of currently open connections.
- `DownloadId` is a UUID string; paths serialize as strings.
- States are `Queued | Downloading | Pausing | Paused | Canceling | Canceled |
  Failed | Completed`.
- `Segment { start, end, committed, downloaded }`; `end: null` denotes a single
  streaming transfer whose length was unknown.

Contracts:

- Enqueue persists before success. Parents must exist. Destinations are never
  implicitly overwritten; `path_exists` is only a UI preflight.
- Pause/cancel drain workers. Cancel retains data and history.
- Resume is a no-op for queued, active, and completed work; failed/canceled work
  can be requeued. Recovery opens interrupted work paused.
- Changing a URL is allowed only for fully paused or failed downloads. It persists
  the new source and leaves the download paused. Resume checks the validator,
  length, and range support before reusing partial data; incompatible sources restart.
- HTTP 429/503 responses keep active downloads waiting and retrying automatically,
  with cancellable host-wide cooldowns and full `Retry-After` waits. Other transient
  failures use the configured retry budget. While waiting, `error` can contain an
  informational recovery message even though the state remains `Downloading`.
- Reorder requires **every currently retained history/queue ID exactly once**.
  After removal, omit the removed ID. Other entries keep their relative order.
- Concurrency ceilings are fixed until the engine is reopened; actual connections
  and request sizes adapt live. Bandwidth can change live.
- Network error strings omit request URLs. Local databases contain source URLs
  and signed query strings and must not be uploaded as diagnostics.

## 2. Tauri commands — available

Command names match `src/lib/bridge/tauri.ts`. The outer command arguments use
camelCase; nested Rust structs retain snake_case. Errors reject with a string.

| Command | Arguments | Result |
| --- | --- | --- |
| `engine_open` | `config: Config` | `()` |
| `engine_shutdown` | — | `()` |
| `engine_enqueue` | `request: { url, destination, expected_sha256?: string }, startPaused?: boolean, properties?: {description, category_id}, credentials?: {username, password}` | UUID string |
| `engine_update_properties` | `id: string, properties: { description, category_id: string \| null }` | `()` |
| `engine_update_destination` | `id: string, destination: string` | `()` |
| `engine_set_credentials` | `id: string, credentials: { username, password } \| null` | `()` |
| `engine_probe` | `url: string` | `ProbeInfo` |
| `engine_inspect_media` | `url: string, allowedOrigins: string[], requestId: UUID, format?: "hls" \| "dash"` | `{kind, choices, segment_count, resource_origins}` |
| `engine_cancel_inspection` | `requestId: UUID` | `()` |
| `engine_create_media_job` | `request: {url, destination, allowed_origins, ffmpeg_path, description, subtitle_url?, category_id?, dash_tracks?, direct_media?, hls_variant_key?, hls_audio_key?, hls_subtitle_key?}` | media job UUID |
| `engine_media_jobs` | — | engine media job snapshots |
| `engine_remove_completed_media_job` | `id: UUID, confirmed: boolean` | `()` |
| `engine_control_media_job` | `id: UUID, action: "pause" \| "resume" \| "cancel"` | `()` |
| `media_offers` | — | session-only `{id, url, title, subtitle_url, kind}[]` |
| `media_dismiss_offer` | `id: UUID` | `()` |
| `engine_create_site_project` | `request: SiteProjectRequest` | project UUID |
| `engine_site_projects` | — | `SiteProject[]` |
| `engine_control_site_project` | `id: UUID, action: "pause" \| "resume" \| "cancel" \| "delete" \| "recheck"` | `()` |
| `engine_queue_site_resources` | `id: UUID, resources: string[]` | child download UUIDs |
| `engine_configure_site_policy` | `id: UUID, policy: SitePolicy, confirmed: boolean` | `()` |
| `engine_update_site_filters` | `id: UUID, extensions: string[], excludedPaths: string[], confirmed: boolean` | `()` |
| `engine_pause`, `engine_resume`, `engine_retry`, `engine_cancel` | `id: string` | `()` |
| `engine_update_url` | `id: string, url: string` | `()` |
| `engine_reorder` | `ids: string[]` | `()` |
| `engine_set_speed_limit` | `bytesPerSecond: number` | `()` |
| `engine_snapshot` | — | `Download[]` |
| `engine_remove_download` | `id: string, removePartialFile: boolean` | `()` |
| `engine_cleanup_partial_data` | — | number of files removed |
| `pick_directory` | `title: string, defaultPath?: string` | path or `null` |
| `default_download_directory` | — | absolute existing folder |
| `path_exists` | `path: string` | boolean |
| `open_path` | `path: string` | `()` |
| `reveal_path` | `path: string` | `()` |
| `settings_load` | — | `AppSettings` or `null` |
| `settings_save` | `settings: AppSettings` | `()` |
| `browser_setup_status` | — | `BrowserSetupStatus` |
| `browser_setup_register` | `browser: "chrome" \| "edge" \| "brave" \| "chromium" \| "firefox", extensionId: string, confirmed: boolean` | `()` |
| `browser_setup_set_identities` | `browser: "chrome" \| "edge" \| "brave" \| "chromium" \| "firefox", extensionIds: string[], confirmed: boolean` | `()` |
| `browser_setup_unregister` | `browser: "chrome" \| "edge" \| "brave" \| "chromium" \| "firefox", confirmed: boolean` | `()` |
| `browser_setup_open_store` | `browser: "chrome" \| "edge" \| "brave" \| "chromium" \| "firefox"` | `()` |
| `browser_setup_open_files` | `browser: "chrome" \| "edge" \| "brave" \| "chromium" \| "firefox"` | `()` |
| `updates_check` | — | `()`; opens the native update flow |
| `test_sound` | `path: string` | `()`; validates and plays a Windows WAV |
| `drag_completed` | `id: string` | `()`; native copy drag of an existing completed file |

`engine_open` is idempotent inside one process (including StrictMode and webview
reloads). Persisted native settings take precedence over the supplied config;
config applies only when opening a new engine. A separate process holding the
engine database lock causes an error. Startup errors are shown in the UI banner.

Paths supplied to filesystem commands must be absolute. Open/reveal resolve an
existing filesystem path and use the OS handler/Explorer through the opener
plugin. Native dialogs return `null` on cancellation.
`default_download_directory` returns the OS download folder when it exists and
falls back to the user home directory, so the add-download dialog never starts
from a placeholder path that does not exist.

### Replace an expired download link

Select a paused or failed download, open its inspector, and choose **Change URL**
under **Manifest**. Paste the replacement HTTP(S) link and choose **Save URL**,
then **Resume**. The destination stays the same. The engine checks whether saved
bytes still belong to the replacement source before continuing.

### URL probe — available

`engine_probe` runs the transfer's one-byte ranged request without creating a
queue entry. It follows redirects and returns:

| Field | Type | Meaning |
| --- | --- | --- |
| `final_url` | string | URL after redirects; used for the link-derived fallback name |
| `filename` | string or null | `Content-Disposition` filename (RFC 5987 extended values decoded, path components removed) |
| `content_type` | string or null | MIME type before any `; charset=...` parameter |
| `total_bytes` | number or null | Total length when the server reports one |
| `ranged` | boolean | Whether the server honored the one-byte range request |

The probe does not reserve a destination, write a file, or alter history. The
desktop workflow sanitizes `filename` for the target filesystem and prefers it
over the URL path. If the probe rejects, the UI keeps the link-derived name and
lets the user explicitly continue; the engine's enqueue and transfer still
perform their own validation and retries.

### Events — available

| Event | Payload | Behavior |
| --- | --- | --- |
| `engine://snapshot` | `Download[]` | Full snapshots, changed snapshots forwarded at up to 4 Hz |
| `engine://error` | string | Persistence/scheduler/desktop failures |
| `media://offers-changed` | `()` | Session-only browser offers changed; reread `media_offers` |

On Rust broadcast lag the shell obtains a fresh engine snapshot. The frontend
also refreshes every five seconds. Unsubscribe remains safe when called before
asynchronous listener registration finishes.

### P3 media workflow

**Save media** accepts an explicit HLS source or a direct media file. HLS master
choices show manifest-declared resolution/bandwidth/codecs, never measured quality
or an automatically selected variant. A selected variant must be inspected again
before saving for standalone capture. Masters with associated renditions retain
master context and explicit variant/group choices through admission instead.
Additional redirect/resource origins require explicit approval;
neither manifests nor browser offers authorize credential forwarding.
Choice `bandwidth` is decimal integer text (bits/second), preserving the exact
manifest u64 value beyond JavaScript's safe-integer range. `resolution` is a
two-number tuple or null; codecs are optional manifest-declared text.

`format` selects HLS or conservative static-DASH inspection. DASH choices carry
`track_key`, `track_kind` (`video`/`audio`/`subtitle`), `language` and
`segment_count`; the manifest itself is never offered as a direct file. Sending a
master playlist without an explicit `hls_variant_key` to `engine_create_media_job`
is rejected, as is any segment,
subtitle or redirect origin that was not explicitly approved.

Media jobs support standalone unencrypted MPEG-TS HLS VOD and static fragmented-
MP4 DASH `SegmentList` tracks merged into MP4 or Matroska, with optional validated
SRT/WebVTT embedding. DASH requires at least one explicitly selected
`dash_tracks` key (video and/or audio); its initialization segment is kept first.
Live, event, encrypted/DRM, byte-range, HLS initialization-map, DASH
template/timeline and multi-period sources are rejected rather
than silently altered. Job `sha256` is locally computed, not publisher-
authenticated integrity.

### Guided browser setup

Onboarding, Settings and Capture & queues expose the same explicit setup checklist.
`BrowserSetupStatus` is `{registration_supported, helper_available, capture_enabled,
last_handshake, browsers}`. `last_handshake` is nullable Unix seconds, updated only
after an authenticated, enabled, engine-ready native `ping`; it is session-wide,
not an installed-extension detector or per-browser connection assertion. Browser
entries contain `{browser, name, store_url, extension_id, extension_ids, registered, conflict,
unpacked_available}`; IDs are configured staged identities (including the stable
public-key unpacked fallback); store URLs exist only for configured store listings.
Neither is an installed-extension detector.
`extension_ids` is the owned native-host allowlist, not a scanned browser/profile
inventory. `registered` means that an owned registration targets this build's
packaged helper and can serve those IDs. The list editor replaces 1–32 unique
Chromium IDs after fresh confirmation; Firefox accepts only `osdm@osdm.app`.
An explicitly confirmed empty list unregisters the owned default value. Add/edit/
remove never changes another browser or uninstalls an extension; foreign manifests
are still refused. Ordinary default registration continues to append/preserve IDs.

Registration/removal require explicit confirmation and use only fixed Windows
HKCU Chrome/Edge/Brave/Chromium/Firefox host keys and packaged helper paths. Chromium IDs are
validated; Firefox uses `osdm@osdm.app`. Existing foreign/modified registrations
are never overwritten. Removal deletes only the owned default value, retaining
files, other values and browsers. Manifests are persisted/synced before registration
and use create-new, content-hash paths. Isolated native tests block mutations and
external launches and do not read actual host keys. The browser mock refuses native
setup rather than fabricating results. Upgrades can repair old OSDM helper paths
and migrate exact legacy-script manifests; reconnect preserves up to 32 authorized
Chromium IDs. NSIS invokes a headless path before engine/single-instance startup:
interactive install requires consent, silent repair only touches existing owned
registrations, and uninstall removes only owned values targeting this installation.
See [browser setup](browser-setup.md).

Store/file commands accept a browser enum, never a URL, registry path or executable
from the webview. Store URLs are derived from optional build-time official listing
metadata; without it the UI gives manual packaged-file instructions. The user must
approve extension installation in the browser. Registration does not enable
capture or automatic interception; those are separate explicit controls.

### Media inspection lifecycle

Inspection IDs support cancellation, including a bounded cancellation tombstone
when cancellation arrives before native admission. Closing/changing the dialog
invalidates late results. Media job controls delegate to the engine, and job
counts/hashes/status come only from engine snapshots. The web mock explicitly
rejects network media inspection/download rather than fabricating quality,
segment progress or merge integrity.

The extension's authenticated `offer_media` operation only queues a bounded,
session-only source/title/optional-subtitle offer and shows the desktop window.
It never downloads until the user chooses quality and destination in the app.
Offers contain secret-bearing URLs and must not be logged or persisted as
analytics. Disabled/missing helpers leave browser playback untouched. The popup
uses existing active-tab/scripting permissions and requires an explicit scan and
single-item choice; global and exact-site media detection toggles are independent
of automatic ordinary-download capture.

## 3. Native shell behavior — available

- **Single instance:** second launches focus/show the existing window. The
  database's OS lock additionally protects against other engine owners.
- **Close to tray:** closing the main window hides it. Transfers remain owned by
  Rust. The tray offers Open OSDM, Check for updates, and Quit.
- **Coordinated Quit:** shutdown stops scheduling, drains workers, persists the
  queue, and releases ownership before process exit. Shutdown failures keep the
  app running and show an error. New engine commands are refused during Quit.
- **Notifications:** new transitions to Completed notify once per process.
  Historical completed entries do not notify on startup; failed downloads never
  produce a success notification. OS notification permissions still apply.
- **Settings:** `settings.db` lives in Tauri's per-user application-data
  directory alongside `downloads.db`. A typed, versioned SQLite store commits
  settings atomically with FULL synchronous mode. Only documented non-secret
  settings fields are accepted. Native settings restore before engine startup
  and survive localStorage loss. On first native launch, existing frontend
  settings seed the store. Subsequent changes are mirrored through the bridge.
  Concurrency changes apply on restart; speed changes use the live command.

### Updates — GitHub by default, signed manifests optional

Release builds check the public GitHub Releases API after launch and every six
hours, or when the user selects **Check now** / the tray action. Checks use the
saved `include_prereleases` setting, choose the highest newer semver release, and
require native confirmation before downloading. `updates_check` triggers the same
manual check without blocking the UI.

The GitHub path requires an HTTPS asset from `github.com` or
`*.githubusercontent.com`, downloads the platform package, and verifies its exact
`SHA256SUMS.txt` entry (plus the API asset digest when present). On Windows the
installer runs with `/UPDATE /P /R`, which upgrades in place without uninstalling
and restarts OSDM. On Linux the verified `.deb` opens in the software installer.
The queue is drained and saved before the Windows app exits. No update installs
without consent.

If `OSDM_UPDATE_ENDPOINT` and `OSDM_UPDATE_PUBKEY` are set at compile time, the
signed manifest path takes precedence. The endpoint is an HTTPS Tauri JSON
manifest without query/fragment/credentials, and `<endpoint>.minisig` must hold a
Minisign signature of the exact manifest bytes. Both the manifest and artifact
signature are verified before installation.

The GitHub checksum is not a cryptographic publisher signature. Signing-key
custody, signed release publication, and live signed-update validation remain
publisher release gates.

## 4. P1 engine additions — available

### Download Later

`Engine::enqueue_paused(request)` validates and persists a new entry in `Paused`
state under the same lock used for admission. It never becomes runnable before
an explicit resume. It reserves the destination and retains checksum requirements,
survives restart, and creates neither a network request nor a temporary file.
No schema change is needed: `Paused` is an existing durable state.

Both bridge implementations accept `enqueue(request, startPaused = false)`.
The native command's optional outer `startPaused` argument defaults to false for
existing callers. The add dialog's inspection step may already have fetched
metadata; deferring the enqueue itself does not fetch anything.

Column sorting changes presentation only; the `#` control restores durable queue
order. Unknown sizes/ETAs sort last in both directions. Clear completed history
removes completed records while keeping published files. Bulk actions attempt
every entry, report partial failures, and refresh the engine snapshot.

### `Engine::remove_download(id, remove_partial_file)`

- Only Failed, Canceled, and Completed entries with **no running workers** may
  be removed. Queued, Paused, Downloading, Pausing, and Canceling are rejected.
- `true` unlinks only the expected `.osdm-<id>.part` path in the destination
  directory. Mismatched paths, symlinks, and non-regular files are rejected.
  Published destination names are never removed, even when both names identify
  the same file after interrupted hard-link publication.
- `false` retains the partial and atomically transfers its ownership record to
  an orphan ledger while deleting history. The ledger survives restart.
- Remaining ordering is preserved and persisted positions are compacted, so
  later enqueue/reorder/reopen operations remain deterministic.
- Filesystem deletion precedes history deletion when `true`: if the later DB
  operation fails, history can remain although the requested partial was removed.
  The error is returned; retry removal is safe when the partial is already absent.

### `Engine::cleanup_partial_data()`

- Explicitly removes partials of fully stopped Failed/Canceled entries and
  entryless files recorded in the orphan ledger; returns the count actually
  removed. Repeated cleanup returns zero when nothing remains.
- Never scans arbitrary directories or deletes an unrecorded `.part` file.
- Does not touch queued/paused/active work or published destination names.
- Invalidates recovery/publication progress before deleting retained partials.
  These entries remain in history and can restart safely.
- On an error, earlier successful deletions remain applied; the command rejects
  with the failure and can be retried.

### Persistence migration

Engine schema **2** added `orphaned_partials`. Revision 0/1 databases migrate in
one transaction without changing serialized Download records or queue order.
Newer schema revisions are rejected. Revision-1 application binaries reject
revision 2 rather than silently discarding orphan ownership; downgrade is not
supported. Back up application data before manually reverting binaries.

### Optional capability preflight

`engine_capabilities` remains **not implemented**, and the UI does not call it.
Hard-link publication still requires NTFS/ext4/APFS or another supporting
filesystem; actual filesystem errors remain authoritative. No unsupported
capability result is fabricated.

## 5. Verification and remaining release gates

Automated coverage includes real Tauri mock-runtime IPC dispatch using frontend
payload names, startup/shutdown and locking, native settings reopen/validation,
history removal, cleanup ownership, no-overwrite behavior, queue reordering, and
revision-1 migration. Run:

```text
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Native interactive checks remain distinct from mock-runtime tests: native folder
dialogs, Explorer/open handlers, close-to-tray and Quit, second-launch focus,
notification permissions, signed production updates, and installer
install/upgrade/uninstall must be exercised on supported Windows installations.
Accessibility validation remains separate P1 release work.

### Category destination suggestions

Settings now include `categories` (ordered `{ id, name, extensions, domains,
directory }` records) and `remember_last_directory`. Existing settings receive
the five built-in categories and remember-folder defaults on load. Native
settings retain their existing JSON storage schema; older applications reject
these unknown fields rather than silently discarding the rules. Browser mock
mode stores the same settings through Zustand/localStorage.

The add dialog routes the confirmed filename using the original URL's exact,
case-insensitive hostname first, then case-insensitive extension suffixes
(including compound extensions). First category in settings order wins a tie.
Domain rules do not inspect credentials, paths, queries, redirects, or subdomains.
Unknown types use General and the default folder. An empty category folder uses
the default folder; the user can select a category or override its folder before
enqueueing. Category directories must already exist. Category folders do not
replace the remembered general folder. The engine receives the confirmed absolute
destination and retains final conflict/no-overwrite authority.

Editing, reordering, or deleting categories affects future suggestions only;
existing files are untouched. Run `pnpm test` for routing precedence, suffix
matching, and deletion fallback regressions. Category assignments are now stored
in each download's `properties` and exposed in category navigation; deleting a
rule preserves existing assignments under a deleted-category entry.

## 6. P1 daily-use contract

`Download.properties` contains `{ description: string, category_id: string | null }`.
Descriptions are limited to 4096 UTF-8 bytes; category IDs to 128 bytes.
`enqueue_with_properties` persists metadata with admission, rather than updating
an already-runnable job afterward. `update_properties` persists a clone before
changing the live snapshot and is independent of transfer state. Descriptions
participate in search. Category edits never move downloaded files.

`update_destination` accepts stopped Paused/Failed/Canceled entries, rejects
publication recovery, existing names, and concurrent reservations, and persists
before exposing the new path. A same-folder rename retains the working file;
folder changes are allowed only before any layout or partial exists. Completed
files and active work cannot be relocated through this command.

Optional Basic credentials are input-only, scoped to the requested download's
original origin and held in engine memory. Cross-origin redirects strip
Authorization; requests to redirected origins do not receive the original secret.
HTTPS is required outside loopback. Pause/failed credentials can be replaced or
cleared. URL replacement, completion/cancel, removal, and shutdown clear secrets;
restart requires reentry. They are not part of `Download` or native settings.

Engine schema **4** adds defaulted properties and optional canonical
`temporary_directory`, including in orphan ownership records. Revisions 0–3
migrate transactionally; revision-3 binaries reject revision 4. Existing history
receives empty descriptions, General category, and destination-local storage.

`Settings.temporary_directory` applies only to new entries. A bounded hard-link
preflight rejects unsupported/cross-volume directories. With custom storage,
the engine persists publication intent, links the verified working file to
`.osdm-<id>.publish.part` beside the destination, then performs final publication
as a same-directory, no-overwrite hard link. Cleanup validates owned names and
staging identity, rejects unrelated collisions/symlinks, and never removes the
published destination. The orphan ledger retains both names across history removal.

`Settings.desktop` defaults to explicit confirmation, completion dialogs enabled,
and progress dialogs/sound/scanners/silent mode disabled. Its fields are `silent`,
`completion_dialog`, `progress_dialog`, `minimized_progress`, `confirm_downloads`,
`sound_file`, `scanner_enabled`, `scanner_executable`, `scanner_arguments`.
`visible_columns`, `toolbar_actions`, and `onboarding_complete` are also persisted.
Native settings remain authoritative; browser settings mirror the same shape.

Immediate start follows a successful metadata probe; inspection errors and path
conflicts still require review. Native scanner tasks are opt-in, have literal
arguments (only an exact `{file}` is substituted), bounded concurrency and a
five-minute kill-on-drop timeout, and do not gate engine integrity/publication.
Failures are reported, never described as malware verdicts. Windows WAV tests
validate RIFF/WAVE headers and a 10 MiB bound. Native drag-out validates the engine
ID, Completed state, and existing regular file, and uses Copy mode only.

See `docs/p1-validation.md` for restrictions and native checks that automated
bridge/browser tests do not replace. Localization is skipped by user decision.

## 7. P2 capture and automation — source implementation

The transfer seam remains `download-engine`. Both bridge adapters expose:

| Command | Arguments | Result |
| --- | --- | --- |
| `engine_enqueue_batch` | `items: {request: DownloadRequest, queue_id: UUID \| null}[], startPaused: boolean` | `{index,id: UUID \| null,duplicate,error: string \| null}[]` |
| `engine_automation` | — | `Automation` |
| `engine_configure_automation` | `command: AutomationCommand` | `Automation` |
| `capture_status` / `capture_enable` | — / `enabled: boolean` | boolean / `()` |
| `desktop_autostart_status` / `desktop_autostart` | — / `enabled: boolean` | boolean / `()` |
| `power_status` | — | `{queue_id,action,deadline: UTC seconds \| null}` or null |
| `power_arm` | `queueId: UUID, action: {kind:"shutdown"} \| {kind:"disconnect",connection:string}, consent: true` | `()` |
| `power_cancel` | — | `()` |
| `power_acknowledge` | — | `()`; visible countdown UI heartbeat; hidden/minimized or unresponsive UI cancels the action |

`Automation` contains `queues`, `schedules`, download→queue `membership`, internal
download→expiry `pending_captures`, and revision→source `sync_roots`. Queue fields
are `{id,name,running,stop_at,synchronize,checking_sources,last_error}`. Schedule
fields are `{id,queue_id,next_run,interval_seconds,stop_after_seconds,
run_once_on_startup,enabled}`. IDs are UUIDs, time fields UTC seconds. Commands use
an `action` discriminator: `save_queue {id,name}`, `delete_queue {id}`,
`assign {ids,queue_id}`, `set_synchronization {id,enabled}`,
`save_schedule {schedule}`, `delete_schedule {id}`, `start {id}`, `stop {id}`.

Queue/schedule writes and all affected download states commit in one SQLite
transaction before live mutation, cancellation, or transfer admission. Schema
**5** migrates revisions 0–4 transactionally and forces older binaries to reject
the database rather than bypass schedules/capture intent. Existing entries remain
in General. Source URLs stay in Download records; automation contains only IDs,
policy, timing, and safe status text. Duplicate identity is normalized URL and
canonical destination/checksum, not filename/size or inferred content identity.
Batch admits 1–1,000 items independently, reports partial failures, and retains an
ID even if a later assignment/resume fails. Retry does not create another entry.

Schema **6** adds the durable media-job ledger and schema **7** adds persisted Site
Grabber projects plus restrictable child downloads. Both migrate revisions 0–5 in
one transaction; older binaries reject revision 6/7 rather than bypass media
publication or site byte/scope guards. A site child persists `max_bytes` and
`deny_redirects` before dispatch, refuses credentials/destination/source rewrites,
skips synchronization, and requires an exact final length. Back up application
data before downgrading binaries.

Schema **8** transactionally adds a site-policy format marker and prevents older
binaries from ignoring unattended policy, revisions, caches or lifetime charges.
Legacy project fields default to manual review with no cache or schedule.
Unknown policy formats and newer database versions are rejected.

Schema **9** prevents revision-8 binaries from ignoring durable media cleanup
intent. Completed jobs default `removal_pending: false`; confirmed cleanup persists
true before filesystem work, refuses resume, and stays retryable after restart.
Only fully stopped completed jobs outside automation queues can be cleaned. It
validates ownership markers, planned child identities and content hashes, refuses
foreign/modified files, and removes individual inputs/history/recorded merge anchor
without unlinking published output. A tiny workspace marker is retained. No broad
directory deletion or stale-artifact globbing occurs.

Schema **10** guards persisted HLS stream-role selection and segmented WebVTT.
Revisions 0–9 migrate transactionally; legacy tracks default to the existing
all-stream mapping, and legacy single-file subtitles default to no HLS segments.
Revision-9 binaries must reject the new version rather than ignore these rules.

For an HLS master, choices add stable `track_key`, `hls_audio_group` and
`hls_renditions: {key, kind: "audio"|"subtitle", name, language, in_band, default,
forced}[]`. Manifest flags are labels, never implicit choices or authorization.
Set `url` to the master and supply its selected `hls_variant_key`; a variant with
an audio group requires `hls_audio_key` (a matching key, or the explicit sentinel
`"none"` for video-only). `hls_subtitle_key` is optional and mutually exclusive
with `subtitle_url`. HLS choices cannot be mixed with DASH or direct-media mode.
The engine re-fetches the master and validates current keys/group membership,
selected playlist origins and every segment origin before fetching that resource.
Only complete unencrypted MPEG-TS VOD and segmented plain WebVTT are supported;
selected stream mapping and relative MPEG clock offsets survive restart. Caption
timestamp maps normalize onto the primary video clock; boundary cues deduplicate.
See `docs/p3-retrieval.md` for exact format, clock and budget limits.

### Site Grabber workflow

`engine_create_site_project` persists an explicit, confirmed project (one origin,
directory prefix, extension/exclusion filters, page/file/byte/depth ceilings) as
paused intent. Resume fetches the origin's robots policy first and explores with
one engine-budgeted request at a time; redirects are validated before the next
request, and credentials are never forwarded. `SiteProject.state` moves
`Paused → Exploring → Ready | Failed | Canceled`. Saved projects open and delete
without deleting published files; restart reopens interrupted work paused, except
for a separately authorized schedule's explicit catch-up decision.

Discovered files stay review-only and carry `size: null` until an engine-owned
HEAD returns an exact length; unknown lengths cannot be selected. Queueing a
reviewed set reserves each exact payload and admits an ordinary hard-capped
download through the normal transfer/checkpoint/publication path. The byte limit
is a payload/storage budget (discovery bodies plus reserved file payload), not a
metered WAN retransmission quota, so ordinary resume and retries keep working.
`engine://snapshot` remains authoritative for child progress.

`SitePolicy` is `{automatic_downloads, max_concurrent_downloads, cache_responses,
next_run, interval_seconds, catch_up_on_startup}` (snake_case). UTC times are Unix
seconds; intervals are 60–31,536,000 seconds, concurrency is 1–32. Null `next_run`
disables the schedule and requires null interval. Schedule slots persist before
dispatch; overlaps skip, missed startup slots skip unless catch-up is enabled.
Command-only engine owners never dispatch schedules. Pause/Cancel clears them.
Automatic downloading applies after exploration, only to known-size matching
resources that fit the remaining lifetime byte budget.

`SiteProject` adds `policy`, `generation`, `last_run`, `schedule_note`, `extensions`
and `excluded_paths`; resource entries add bounded plain-text `description`.
Recheck retains prior files and byte charges, uses distinct revision filenames
and review IDs, and refuses unfinished child transfers. Filter updates preserve
origin/path, require confirmation and terminal child transfers, save paused, and
disable automatic downloads/scheduling. Conditional HTML cache is always
revalidated, bounded to 16 pages/512 KiB and never serves stale-on-error content.

Media `direct_media` defaults false. True selects a single direct MP4/M4V/MOV
source for local subtitle remux, without DASH selections. Merged destinations
accept `.mkv` or `.mp4`; all modes retain verified no-overwrite publication.
Subtitle format comes from a `.srt`/`.vtt` suffix or a recognized response MIME
(`application/x-subrip`/`text/vtt`); conflicting declarations fail. This permits
extensionless exposed DASH/player tracks without guessing or bypassing bounded
document validation. Manifest-discovered subtitles are never selected implicitly.

Site project templates are scope-free UI presets, not additional engine authority:
they set finite filters/budgets, clear confirmation, and retain no URL/destination,
credentials, schedule or automatic-download consent.

`Engine::prepare_capture` atomically inserts a paused download with a 30-second
lease. Ordinary resume/queue start cannot admit that lease. Commit consumes the
lease and queues the ID in one transaction; abort/expiry removes only a pending,
nonrunnable capture. Captured Cookie/Authorization/User-Agent inputs are validated,
origin-scoped and memory-only, never part of serde Download or settings. They
clear on source replacement, cancel, completion, removal and shutdown. The browser
native host never owns or opens the engine database. Its authenticated endpoint
and protocol are documented in `extension/README.md`.

The native app remains authoritative. UI queues poll engine automation snapshots;
the mock simulates queues/UTC windows but refuses native credentials, browser
registration, remote revision checks and power actions instead of inventing those
results. `shutdown_if_idle` atomically refuses active work, source checks and
pending browser handoffs before quiescing; it is used for opt-in completion actions.
Power actions are session-only and require a visible 60-second countdown. Real
platform execution also requires a fresh visible-window UI heartbeat; hiding,
minimizing, or losing the countdown UI cancels the action rather than shutting
down silently. The main webview disables Tauri's native drop handler so Windows
HTML5 URL drops reach the reviewed desktop capture surface.
Real platform validation and current policy limits: `docs/p2-validation.md`.
