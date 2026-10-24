# OSDM — priority-driven product and engineering plan

## 1. Product

### 1.1 Goal

Build a Windows-first desktop download manager with an independent Rust engine and a TypeScript interface. The defining qualities are reliable segmented HTTP downloads, recoverable progress, predictable resource use, and a polished desktop experience.

Parallel connections can overcome per-connection server limits; they cannot exceed available network, server, or disk capacity. Performance claims must come from measurements.

### 1.2 Principles

- **Engine first.** Rust owns download state, networking, scheduling, disk writes, recovery, and errors. The UI sends commands and renders authoritative snapshots.
- **Correctness before feature count.** Every priority has an exit gate. A feature is not shipped because it exists; it ships when its failure modes are understood and tested.
- **Safe by default.** Never overwrite an existing file, never shut down silently, never delete data as a side effect of canceling.
- **Measured performance.** Publish throughput numbers from repeatable benchmarks, not multipliers.
- **Local-first privacy.** No telemetry by default. Credentials are protected at rest. Signed URLs are treated as secrets.
- **Small dependencies.** Add a library only when it removes more complexity than it introduces.
- **Priority-driven delivery.** Each priority is independently valuable, testable, and safe to stop at.

### 1.3 Users and workflows

Core workflows:

1. Paste a direct download URL, choose a destination, and watch progress.
2. Download several large files while controlling total network consumption.
3. Pause downloads and resume them after closing the application.
4. Recover interrupted work after a process crash without corrupting the result.
5. Understand failed downloads and retry them without losing safe progress.
6. Open completed files or reveal their location from the desktop application.

Later workflows:

7. Capture downloads from a browser without copying URLs by hand.
8. Queue overnight or recurring work and shut down when it finishes.
9. Save ordinary web video and subtitle resources.
10. Mirror a filtered subset of a documentation or media site for offline use.

### 1.4 Non-goals

- Advertised speed multipliers or benchmarking by anecdote.
- MMS, Internet Explorer cache scraping, proprietary skin formats, and dial-up redial as first-class features.
- Bundled telemetry, cloud accounts, or remote control.
- Automatic playlist or site-wide video capture.
- Silent shutdown without explicit opt-in.
- General-purpose scraping or headless browsing inside Site Grabber.
- BitTorrent and cloud-drive synchronization in this plan.

These may be revisited only with a written product decision and a test plan.

## 2. Delivery model

### 2.1 How priorities work

Work is organized by priority rather than release number. Priorities express value and dependency order: higher-priority work is completed and validated before lower-priority work begins, unless a written decision explicitly reprioritizes it.

Each priority:

- has one theme and a short feature list;
- leaves all earlier priorities working;
- includes tests for its failure modes;
- ends at a gate that can be demonstrated without reading code;
- may slip, but is not partially declared complete.

Features inside a priority can ship as they become ready. A priority is complete only when every gate item is satisfied.

### 2.2 Priority map

| Priority                    | Meaning                        | Scope                                                                | Gate                                                             |
| --------------------------- | ------------------------------ | -------------------------------------------------------------------- | ---------------------------------------------------------------- |
| P0 Foundation               | Blocks all other work          | Single transfer, segmented transfer, durable queue and recovery      | Headless engine is reliable under normal and crash conditions    |
| P1 First release            | Must ship for a usable product | Desktop shell, daily-use features, installer, updates                | Public standalone release installs, upgrades, and preserves data |
| P2 Everyday value           | High value after standalone    | Browser capture, batch and command line, scheduling, multiple queues | Capture and unattended workflows are dependable                  |
| P3 Differentiated retrieval | Larger risk and effort         | Video and streaming capture, Site Grabber                            | Media and site retrieval meet their safety gates                 |
| P4 Network breadth          | Expands supported environments | FTP, proxies, authentication, quotas                                 | Proxy and auth matrix passes; secrets are protected              |
| Ongoing                     | Evidence-gated                 | Segmentation, reuse, adaptive tuning, durability                     | Benchmarks justify each change                                   |

### 2.3 Ordering and dependencies

- P0 is the engine foundation. The repository currently contains its implementation; hardening and validation remain.
- P1 cannot begin until the engine bridge contracts have been exercised.
- P2 begins after the first public release. Browser capture precedes video capture; batch and CLI precede scheduling; scheduling precedes scheduled Site Grabber work.
- P3 depends on P2 for capture and scheduling surfaces.
- P4 can run in parallel with P2 and P3 if it does not destabilize the transfer core.
- Ongoing work has no fixed date. It competes with product priorities on measured evidence.
- Reprioritization requires a written rationale, updated gates, and an explicit check that no earlier priority regresses.

### 2.4 Definition of done

A work item is done only when:

- its automated tests pass and cover the documented failure modes;
- tests for earlier priorities still pass;
- user-visible behavior is documented, including what does not work;
- persistence changes include a migration and a downgrade test;
- performance-sensitive changes include before/after measurements;
- secrets, browser permissions, and filesystem writes receive a focused review;
- accessibility and keyboard paths are exercised for UI work;
- the gate result is recorded in the release notes.

## 3. Architecture

### 3.1 Technology

| Layer         | Choice                        | Notes                                                              |
| ------------- | ----------------------------- | ------------------------------------------------------------------ |
| Engine        | Rust, Tokio                   | Streaming networking and cancellable workers                       |
| HTTP          | Reqwest, Rustls               | TLS, redirects, keep-alive, streaming; automatic decoding disabled |
| Persistence   | SQLite, SQLx                  | WAL, FULL synchronous mode, serialized snapshot writes             |
| Desktop shell | Tauri 2                       | Thin bridge and native integrations                                |
| Frontend      | React, TypeScript, Vite       | UI only; no transfer logic                                         |
| Styling       | Tailwind CSS, shadcn/ui       | Consistent visual primitives                                       |
| UI state      | Zustand                       | Engine snapshots and transient selection state                     |
| Testing       | Tokio, Axum, SHA-256 fixtures | Controlled HTTP behavior and exact-output verification             |

Keep the dependency graph modest. Do not introduce an abstraction for every internal function. The public engine interface is the stable seam; transport, scheduling, and storage stay behind it.

### 3.2 Repository layout

```text
Cargo.toml
Cargo.lock
PLAN.md
README.md
src/                         # TypeScript application
src-tauri/                   # Desktop bridge
crates/download-engine/
  Cargo.toml
  README.md
  src/
    lib.rs                   # Public exports and errors
    model.rs                 # Configuration, state, segments, events
    engine.rs                # Public commands, scheduler, worker lifecycle
    transfer.rs              # Probing, transfer, checkpoints, publication
    store.rs                 # SQLite persistence
  tests/downloads.rs         # Local HTTP integration tests
  examples/download.rs       # Headless manual exercise
```

Planned additions, created only when their priority begins:

```text
extension/                   # Browser integration, P2
crates/cli/                  # Command-line contract, P2
crates/media-capture/        # Stream inspection and merging, P3
crates/site-grabber/         # Spider, filters, projects, P3
```

Each addition gets its own README, tests, and a boundary that keeps transfer logic in `download-engine`.

### 3.3 Engine ownership and API

Rust owns download state, network requests, scheduling, disk writes, recovery, and errors. The frontend sends commands and displays authoritative snapshots.

Initial interface:

- `Engine::open(database_path, config)`
- `enqueue(DownloadRequest) -> DownloadId`
- `pause(id)` / `resume(id)` / `retry(id)` / `cancel(id)`
- `reorder(all_ids)`
- `set_speed_limit(bytes_per_second)`
- `snapshot()`
- `subscribe()`
- `shutdown()`

Configuration is validated at startup. Concurrency changes initially require reopening the engine; bandwidth limits can change at runtime. A request specifies an explicit absolute destination filename. Filename suggestions and directory creation belong to the desktop workflow.

Later priorities extend the interface with queue, category, schedule, and project commands rather than bypassing the engine.

### 3.4 Contracts

- An OS file lock enforces one engine owner per database and is released on shutdown or process exit.
- Commands run on a Tokio runtime.
- Enqueue persists before returning success.
- Pause and cancel return after transfer workers have drained; no worker writes afterward.
- Cancel preserves temporary data and history. File and history deletion is a separate command added with the desktop release (P1).
- Resume is a no-op for already queued, active, or completed downloads.
- Shutdown stops scheduling, pauses work, waits for workers, and persists final state.
- Recovered queued or active downloads open paused; the UI can ask whether to resume.
- Progress events are best-effort full snapshots at up to four updates per second. Lagging subscribers refresh with `snapshot()`.
- Persisted source URLs can contain signed query parameters; the database is local application data, not telemetry.
- Public network errors omit URL strings. No automatic telemetry is included.

### 3.5 State machine

```text
Queued -> Downloading -> Completed
              |  |
              |  +-> Failed -> Queued
              +-> Pausing -> Paused -> Queued
              +-> Canceling -> Canceled -> Queued
Queued -> Paused / Canceled
```

Intermediate states distinguish a stop request from fully drained workers. A transfer finishing concurrently with a stop request can already be complete before that request takes effect.

## 4. Core engine design

### 4.1 Probe

Send `GET` with `Range: bytes=0-0` and `Accept-Encoding: identity` instead of depending on HEAD behavior. Validate status, range start and end, total size, and the one-byte body. Capture the effective URL and a strong ETag, or Last-Modified when present. A `200` response means single-stream mode; an empty resource may return `416` with `bytes */0`.

### 4.2 Strategy

Initial defaults:

- Three active downloads.
- Four connections per file, configurable from one to eight.
- Twelve total connections.
- Eight connections per host.
- Split threshold of 16 MiB.
- Three bounded retries.

Ranged transfer additionally requires a usable validator. Without one, use a single stream to avoid combining unverified versions across requests. Last-Modified is weaker than a strong ETag; servers must provide correct validators. The engine cannot independently prove that an untrusted server has not changed content without changing its validators.

### 4.3 Segment layout

Partition `[0, total_size)` into fixed, disjoint ranges. Each segment has a start, exclusive end, live byte count, and durable committed byte count. Completed segments are skipped on resume.

Use one hidden temporary file in the destination directory. Each worker has its own file handle and seeks within its assigned disjoint range. This avoids a merge and keeps segment writes separate without a central writer bottleneck. Streaming and throttling use bounded chunks; file size does not determine buffer size.

### 4.4 Response validation

- Require exact `206` and matching Content-Range for ranged transfers.
- Send If-Range when resuming or requesting segments.
- Reject unexpected content encoding and inconsistent response sizes.
- Never write a full `200` response into a segment.
- If ranges stop working, drain all workers, discard partial state, and retry as one stream.
- A changed resource detected by a new probe causes a fresh transfer.

### 4.5 Checkpoint durability

At each checkpoint:

1. Flush the worker's writes.
2. Sync file data.
3. Update that segment's committed count.
4. Persist its containing download snapshot while holding the state lock.

Checkpoint at approximately 1 MiB per worker and on worker stop. Recovery resets live counts to committed counts. Bytes written after the last durable checkpoint can be overwritten on resume. The ordering deliberately favors safe repeated work over false claims that unsynced bytes are recoverable.

The shared state lock also serializes database snapshots so an older update cannot overwrite a newer one. Benchmark this choice before replacing it with a more complicated persistence pipeline.

### 4.6 Retry policy

Retry network failures, truncated bodies, 408, 429, and server errors with bounded exponential backoff. Honor numeric Retry-After on transfer responses, capped at 120 seconds. Every wait is cancellable. Do not retry disk or permission errors as if they were network errors.

Follow-up hardening: HTTP-date Retry-After and probe-level Retry-After handling, jitter, richer structured failure categories, and optional per-host cooldown shared across downloads.

### 4.7 Persistence

SQLite stores the ID, queue position, and serialized download record. Records include source URL, destination and temporary paths, state, expected size, validator, range layout, committed bytes, and publication intent. The initial schema is revision 1; future record changes need migration tests before desktop upgrades ship. Later priorities add categories, schedules, credential references, and Grabber projects; each addition ships with a migration and a downgrade test.

### 4.8 Publication

After every segment is complete, persist a `ready_to_publish` marker. Publish using a same-directory hard link, which atomically creates the destination without overwriting an existing file. Persist completion, then unlink the temporary name.

If a crash occurs after link creation, recovery checks whether destination and temporary paths identify the same file before accepting the existing destination. A different destination is a conflict, never permission to overwrite it.

This is an intentional deviation from a plain rename: cross-platform rename behavior can overwrite existing files. Initial publication requires a local filesystem supporting hard links, such as NTFS, ext4, or APFS. Supporting FAT/exFAT or remote filesystems requires a separately tested no-clobber publication strategy before claiming support.

Successful length checks establish completeness, not authenticity. An optional expected SHA-256 should be verified before publication. Integration tests already compare SHA-256 against known fixtures.

Process-crash recovery is in scope. Full power-loss durability of directory entries and all filesystem-specific behaviors requires additional platform work and is not claimed by this initial implementation.

### 4.9 Resource management

- Bound active download count and both global and per-host network connections.
- Acquire permits around probes and streamed transfer bodies.
- Segment workers share a global rate reservation clock.
- Respond to cancellation while waiting for requests, permits, throttling, or retries.
- Drain worker tasks before marking a transfer paused or failed.
- Keep source data on disk rather than in whole-file memory buffers.
- Catch task termination at the supervisor so a failed worker cannot permanently occupy a queue slot.

Default redirect handling is bounded to ten hops. Transfer connections use the resolved host; a redirect chain itself is accounted against the original probe host. Strict per-hop redirect accounting is a later hardening item.

## 5. Desktop application

### 5.1 Shell and bridge

The Tauri bridge converts Rust records to serializable command results and subscribes to engine events. A hidden window must not terminate transfers. Application exit must explicitly coordinate engine shutdown. The UI never performs network transfers or writes download data itself.

Later priorities extend the same shell with categories, multiple queues, schedules, capture status, and Grabber projects rather than introducing a second UI paradigm.

### 5.2 Information architecture

- **Sidebar:** all, active, queued, completed, failed, categories, scheduled, Grabber, settings.
- **Toolbar:** add, pause/resume all, stop all, search.
- **Main list:** icon, filename, status, progress, bytes, speed, ETA, actions.
- **Details:** origin, destination, category, segment activity, errors, retry, open/reveal.
- **Status bar:** aggregate throughput, active count, queued count, speed limit.

The list is the primary surface. Dialogs are reserved for actions that need confirmation or detailed input. Bulk actions must be reversible where possible and clearly scoped.

### 5.3 Interaction and visual design

A compact desktop utility with clear typography, restrained animation, and useful information density. Prefer explicit state labels over spinner-only feedback. Empty states explain how to add a download. Errors state what failed, whether progress is safe, and the next action.

### 5.4 Accessibility

Keyboard navigation for every action, visible focus, screen-reader labels for progress and state, scalable text, sufficient contrast in both themes, and no color-only status indicators.

## 6. Priority plans

### 6.1 P0 — Correctness foundation

**Priority goal:** make the headless engine trustworthy before a UI or optional feature depends on it.

#### 6.1.1 Reliable single transfer

**Goal:** prove that the engine can download one file correctly with bounded memory.

Features:

- Manual URL entry through the engine API with an explicit absolute destination.
- HTTP/HTTPS downloads and bounded redirects.
- Streaming to a temporary file with chunked reads and writes.
- Single-stream fallback for unknown length.
- Start and cancel; progress snapshots with bytes, speed, and ETA when length is known.
- Bounded memory use and no whole-file buffering.
- Errors distinguish network, HTTP, disk, permission, and cancellation failures.
- No implicit overwrite; the destination parent must already exist.

**Exit gate:** a large local fixture downloads with an identical hash; memory stays bounded; cancellation leaves no published destination; redirect loops fail cleanly.

**Depends on:** nothing.

#### 6.1.2 Segmented transfer

**Goal:** use parallel connections without corrupting output or lying about progress.

Features:

- Range probe using `GET bytes=0-0`, with strong ETag or Last-Modified validation.
- Fixed-range segmentation above a configurable threshold.
- Configurable connections per file, global connection cap, and per-host cap.
- `206` and Content-Range validation, If-Range on segment requests, and encoding checks.
- Server-compatibility fallback to a single stream when ranges are unsupported or withdrawn.
- Retries with bounded exponential backoff and cancellable waits.
- Per-segment progress and aggregate speed/ETA.
- Changed-validator detection that restarts the transfer safely.

**Exit gate:** output hashes match across range-supporting, range-ignoring, malformed, and interrupted servers; connection caps are honored; cancellation latency is bounded.

**Depends on:** reliable single transfer (P0).

#### 6.1.3 Durable queue and recovery

**Goal:** survive restarts and crashes without corrupting files or claiming false progress.

Features:

- SQLite persistence with WAL and FULL synchronous mode.
- Persistent queue, ordering, active-download limit, and history.
- Pause, resume, retry, cancel, and move-to-front/reorder.
- Startup recovery that reopens interrupted downloads as paused.
- One engine owner per database enforced by an OS file lock.
- Runtime-adjustable global bandwidth limit.
- Durable checkpoints and safe hard-link publication.
- Snapshot events at up to four per second plus on-demand snapshots.

**Exit gate:** graceful and abrupt process exit recovery produce correct files; resume starts from committed offsets; no existing destination is overwritten; rate limiting is observable and does not block cancellation.

**Depends on:** segmented transfer (P0).

**Priority gate:** all three engine gates pass; the headless engine is the stable seam for the desktop bridge.

### 6.2 P1 — Standalone desktop release

**Priority goal:** make the engine usable without a terminal and ship a maintainable standalone product.

#### 6.2.1 Desktop shell and alpha

**Goal:** make the engine usable without a terminal.

Features:

- Tauri bridge for commands, snapshots, and events.
- React and TypeScript shell with URL input, destination picker, filename suggestion, and conflict handling.
- Download list with progress, speed, ETA, size, and state.
- Search, filters, history, and a per-segment detail panel.
- Queue ordering and bulk pause/resume/cancel.
- Settings for output directory, concurrency, bandwidth, and appearance.
- System tray, completion notifications, and open/reveal actions.
- Dark and light themes, keyboard navigation, accessible labels, and useful empty states.
- Hidden-window transfer safety and coordinated shutdown.

**Exit gate:** every engine lifecycle operation is usable and accessible without a terminal; closing the window does not kill active transfers unless the user exits.

**Depends on:** foundation (P0).

#### 6.2.2 Daily-use features

**Goal:** ship a complete, maintainable standalone downloader.

**Download list and properties**

- Download Later: enqueue without starting.
- Stop All, delete selected, delete completed entries, and separate partial-file cleanup from history removal.
- Sortable columns and finished/unfinished groupings.
- Editable URL, destination or filename, description, category, and applicable credentials.
- Download descriptions and a visible per-segment progress view.
- Open and reveal completed files.

**Organization**

- Automatic categories for Music, Video, Programs, Documents, and Compressed.
- Create, edit, and delete custom categories.
- Extension-to-category mapping and per-category default directories.
- Domain-based categories.
- Destination suggestion from category and file type.
- Configurable temporary directory and optional remembered last-used folder.
- Deterministic routing: conflicting rules resolve in a documented order, and unknown extensions fall back to a default category.
- Category tree that later exposes queues and Grabber projects.

**Dialogs, completion, and safety**

- Show or hide the start dialog, progress dialog, completion dialog, and minimized progress mode.
- Desktop silent mode, immediate-start option, and resume modification-time override for eligible downloads.
- Completion dialog with open and drag-out actions.
- Configurable download-event sounds with custom, testable sound files.
- External antivirus scan with an explicit executable and arguments. This launches a scanner; it is not a guarantee that files are malware-free.
- Onboarding connection check.

**Interface and maintenance**

- Dark theme, toolbar button selection and order, visible columns, and multiple interface languages.
- Toolbar layout and themes come first; a custom skin format is optional and only if demand justifies the parser and asset surface.
- System-tray operation, Quick Update with release notes and notifications, and preservation of settings, queue, and history across supported updates.
- Quick Update verifies signed manifests, refuses downgrades, and never installs without user consent.
- A registration or licensing surface exists only if the product adopts licensing; otherwise it is omitted.

#### 6.2.3 Release engineering

- Windows installer, uninstall, and signing decision.
- Schema migration tests and incompatible-schema rejection.
- Filesystem compatibility checks for hard-link publication.
- Long-running and fault-injection tests.
- Resource benchmarks recorded before release.

**Exit gate:** install, upgrade, and uninstall work on supported Windows versions; categories route files correctly; updates preserve settings, queue, and history; reset and cleanup paths are explicit.

**Depends on:** desktop shell and daily-use features (P1). This exit gate is the P1 gate.

### 6.3 P2 — Capture and automation

**Priority goal:** remove manual URL handling and let work run unattended.

#### 6.3.1 Browser capture

**Goal:** remove manual URL copying for ordinary browser downloads.

Features:

- Browser extension connected through native messaging or an authenticated loopback channel.
- Automatic interception of eligible downloads.
- Right-click “Download with OSDM”.
- Capture a selected link, links in selected text, or all links on the page.
- Clipboard URL detection.
- Drag-and-drop URL capture and drag-out of completed files.
- Advanced integration for applications that launch URLs.
- Custom browser registration by executable and start-with-Windows support.
- Interception modifier keys, per-site exclusions, and extension-based file-type rules.
- Duplicate suppression: a URL already queued from the same page is not added twice; browser cancellation leaves no phantom entry.
- Cookie or authorization handoff only for the requested transfer and only after explicit user action; no bulk cookie export.
- Graceful fallback: if the extension is missing, outdated, or disconnected, the browser's own downloader continues to work.
- Separate Chromium and Firefox test targets, including extension reconnect after restart.

**Exit gate:** supported browsers intercept correctly; exclusions hold; duplicates are suppressed; browser updates degrade to no interception rather than broken downloads; no credentials leak into logs.

**Depends on:** first release (P1).

#### 6.3.2 Batch and command line

**Goal:** make bulk entry and automation reliable.

Features:

- Batch URL entry in the desktop application.
- Clipboard batch capture and import of link lists from a file.
- Stable CLI with URL, destination directory, filename, silent mode, and queue-only mode.
- CLI start-queue and connection-close behavior where applicable.
- Documented exit codes and machine-readable errors.
- Batch-script examples and a contract suitable for automation.
- Command success means the request is persisted, matching engine enqueue semantics.
- Idempotency and duplicate handling for repeated script runs.

**Exit gate:** a script can queue one or many downloads, choose destinations and names, observe failures through exit codes, and run without prompts.

**Depends on:** first release (P1).

#### 6.3.3 Scheduling and queues

**Goal:** run recurring download and synchronization work without a user present.

Features:

- Multiple named queues and queue selection for deferred or batch downloads.
- Start Now, Start Queue, and Stop Queue.
- Scheduled start and stop, recurring intervals, and an end-of-day stop limit.
- One-time jobs and manual starts.
- Periodic synchronization of selected remote files, replacing local copies when the remote changes.
- Multiple independent schedules and an external trigger to start a saved queue, including from the command line.
- Post-download disconnect or shutdown with explicit opt-in and a visible, cancellable countdown.
- Connection scheduling only where the operating system exposes it; no dial-up redial engine.
- A regular download queue processes files as tasks; a synchronization queue persists after a run so later checks can reuse it.

**Policy requirements**

- Persist schedules and define missed-run behavior. Skipping is the default; optional run-once-on-startup is a setting.
- Handle DST and timezone changes without duplicating or dropping runs.
- Define behavior for sleep/resume, overlapping jobs, and clock jumps.
- Never shut down while a user-initiated download is active without an explicit warning.

**Exit gate:** schedules survive restart, sleep/resume, and clock changes; shutdown and disconnect actions are cancellable; synchronization detects changes without corrupting local files.

**Depends on:** batch and command line (P2).

**Priority gate:** browser capture, batch and CLI, and scheduled queues all meet their gates without destabilizing the engine.

### 6.4 P3 — Differentiated retrieval

**Priority goal:** add media and site retrieval with explicit safety boundaries.

#### 6.4.1 Video and streaming capture

**Goal:** save web media.

Features:

- “Download this video” panel when supported media is detected.
- Format and quality selection; capture during playback.
- Background and concurrent video downloads.
- Filename from the page title when the server supplies none.
- Editable destination, description, and category.
- Subtitle capture in SRT or VTT when the source exposes it.
- Player detection without manual URL hunting.
- Per-site panel configuration and global or per-site disable.
- Direct media files and HLS/DASH where manifests and segments are accessible.
- Segment merging with the same checkpoint, integrity, and no-overwrite rules as ordinary downloads.

**Limits**

- No automatic grabbing of every video on a page or an entire playlist; each item is an explicit choice.
- Site adapters are fragile and maintained per site; unsupported sites are expected, not exceptional.

**Exit gate:** quality selection matches the manifest; merged output hashes match; subtitles attach correctly.

**Depends on:** browser capture (P2).

#### 6.4.2 Site Grabber

**Goal:** retrieve a filtered, resumable subset of a website for offline use.

Features:

- Website exploration with page and file filters.
- Full or partial mirroring, with file-type discovery for images, audio, documents, and other resources.
- Project templates and a four-step wizard.
- Review discovered resources before downloading.
- Selective or automatic download, and transfer of selected resources to the main list.
- Project persistence and continuation with changed filters.
- Saved-project management: open, schedule, and delete.
- Concurrency settings and detected-resource descriptions.
- Scheduled exploration and periodic synchronization.
- Optional reuse of cached responses using OSDM's own HTTP cache semantics.
- Guardrails: explicit scope confirmation before a large mirror; configurable page, file, and byte budgets.
- No headless-browser emulation initially. JavaScript-heavy and authenticated sites are supported only within explicit project limits.

**Exit gate:** a mirror can be paused and resumed; it stays within budgets, respects filters, terminates on redirect loops, and does not revisit the same URL indefinitely.

**Depends on:** scheduling and queues (P2).

### 6.5 P4 — Network breadth

**Priority goal:** support more real-world connection environments without weakening the core.

#### 6.5.1 Network and protocol expansion

**Goal:** cover the connection environments serious download users encounter.

Features:

- FTP with passive mode, using a separate stack and test matrix.
- Proxy support: HTTP, HTTPS, SOCKS, PAC or auto-configuration, system proxy import, and browser-request proxy reuse.
- Per-protocol proxy settings, bypass lists, and proxy authentication.
- Site logins: stored credentials, directory-specific credentials, and manual authorization when adding a URL.
- Reuse of the captured browser user agent and support for a custom user agent.
- Per-server connection counts, server or URL exclusions, and connection timeouts.
- Download quotas with automatic resumption in the next permitted window.
- Authentication support for Basic, Negotiate, NTLM, and Kerberos where the HTTP stack and platform support them, verified per proxy and origin.
- Dial-up and VPN behavior delegated to Windows networking. “Connect at scheduled time,” if added, is a thin OS integration.
- Credentials stored through Windows Credential Manager or DPAPI-backed encryption, never plaintext in SQLite.
- Credential and signed-query-string redaction in logs and error surfaces.

**Exit gate:** the proxy and authentication matrix passes for direct, proxied, and authenticated origins; credentials are encrypted at rest; quota and exclusion behavior is observable and cancellable.

**Depends on:** first release (P1).

**Priority gate:** the P4 exit gate passes and behavior from P0–P3 is unchanged.

### 6.6 Ongoing — Engine optimization

**Goal:** improve transfer efficiency only where measurements justify complexity.

Candidates:

- Dynamic segmentation and rebalancing of the largest remaining segment.
- Connection reuse and keep-alive tuning across queued downloads.
- Adaptive concurrency based on observed throughput and server limits.
- Smoother speed and ETA estimation.
- Retry jitter, HTTP-date Retry-After, and shared per-host cooldown.
- Optional expected SHA-256 verification before publication.
- Stronger power-loss and filesystem durability work.
- Memory and disk-throughput optimizations for multi-gigabyte files.

**Gate:** every candidate needs a repeatable benchmark, a correctness test, and a rollback plan. No feature is added because another download manager has it.

## 7. Security and privacy

- The database contains full source URLs, including signed query strings. It is local application data and must not be published or uploaded.
- Credentials use the platform credential store or DPAPI-backed encryption. Nothing sensitive is stored in plaintext configuration files.
- Logs and public errors redact credentials, cookies, and signed query parameters.
- Browser cookie or authorization handoff is scoped to one requested transfer and requires explicit user action.
- Update manifests are signed, downgrades are refused, and installation requires user consent.
- Antivirus integration launches a configured scanner; it does not guarantee that downloaded files are safe.
- No telemetry is sent. Any future diagnostic reporting must be opt-in and previewable.
- Downloaded content is untrusted. The UI avoids executing or rendering active content from downloads.

## 8. Verification strategy

### 8.1 Automated engine coverage

- Correct segmented range coverage and output hashes.
- Range ignored, unknown length, and range support withdrawn after probing.
- Pause stability, reopen, and offset-aware resume.
- Changed validators and changed content.
- Rate limiting and dropped connections.
- Incorrect Content-Range rejection.
- Queue order, concurrency behavior, cancellation, and duplicate destinations.
- Existing destination appearing mid-download.
- Empty files and redirects.
- Abrupt process-exit recovery.

### 8.2 Fault and platform validation

- Disk-full and filesystem permission fault injection.
- Slow or stalled servers, TLS errors, many redirects, and expired signed URLs.
- Missing validators, weak ETags, and Last-Modified precision limitations.
- Multi-gigabyte fixtures and extended queued sessions.
- Cancellation during each publication and checkpoint stage.
- SQLite corruption and incompatible schema rejection.
- Windows long paths, case-insensitive conflicts, Unicode names, and filesystem support.
- Subscriber lag, UI reconnect, and shutdown under load.

### 8.3 Feature validation by priority

- **P1 — Categories and organization:** extension and domain rule precedence, conflicting matches, destination collisions, and category deletion with existing files.
- **P1 — Updates:** signed-manifest rejection, interrupted update, downgrade refusal, and preservation of settings, queue, and history.
- **P1 — Accessibility:** keyboard-only operation, focus order, screen-reader labels, contrast, and scaling.
- **P2 — Browser:** Chromium and Firefox interception, duplicate suppression, exclusions, extension reconnect, browser-update breakage, and fallback.
- **P2 — Scheduler:** restart, sleep/resume, DST and timezone changes, missed runs, overlapping jobs, and a cancellable shutdown countdown.
- **P3 — Video:** HLS/DASH manifests, quality selection, subtitle attachment, merge integrity.
- **P3 — Site Grabber:** page/file/byte budgets, resume, redirect loops, and authentication boundaries.
- **P4 — Network:** proxy type matrix, PAC, proxy authentication, NTLM/Negotiate/Kerberos origins, quota resumption, and credential-at-rest inspection.

### 8.4 Performance measurements

Compare one, four, and eight connections against a controlled per-connection-throttled server and representative remote servers. Record throughput, CPU, peak RSS, disk throughput, startup time, cancellation latency, and resume overhead. Keep payload size and network conditions fixed across comparisons.

### 8.5 Developer checks

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Frontend checks are added when the desktop shell exists: type checking, unit tests for state transitions, and an end-to-end smoke test against a local fixture server.

## 9. Release engineering

- Build signed Windows installers and document the signing identity.
- Keep an update channel with signed manifests, release notes, and rollback instructions.
- Test install, upgrade, and uninstall over an existing installation, including settings and queue preservation.
- Test schema migrations in both directions where downgrade is supported.
- Record supported filesystems and explicitly reject unsupported publication targets.
- Run long-duration soak tests before each release that changes scheduling, capture, or concurrency.
- Keep crash and error reports local unless the user explicitly exports them.

## 10. Risks and open decisions

| Area              | Risk                                                | Decision needed                                                   |
| ----------------- | --------------------------------------------------- | ----------------------------------------------------------------- |
| Browser bridge    | Store review, Manifest V3 suspension, browser churn | Native messaging vs authenticated loopback; first browser targets |
| Video             | Fragile site adapters and codec licensing           | HLS/DASH implementation and licensing; adapter maintenance model  |
| Site Grabber      | Runaway scope and storage growth                    | Crawl library vs custom spider; robots policy; default budgets    |
| Network           | Proxy/auth matrix is large                          | FTP stack, proxy test matrix, credential-store integration        |
| Updates           | A bad update can destroy trust                      | Signing keys, rollout policy, rollback path                       |
| Engine complexity | Dynamic segmentation may not pay off                | Benchmarks required before admission                              |
| UI scope          | Custom skins and licensing add maintenance          | Whether either is in scope at all                                 |
| Scheduling        | Missed runs and sleep behavior confuse users        | Default missed-run policy and shutdown safeguards                 |

Each decision belongs in a short design note, not in code discovered by accident.

## 11. Current status and next steps

The repository currently contains the headless engine and its tests, covering the P0 transfer foundation. The desktop shell and P1–P4 features are planned but not implemented.

Immediate work:

1. Harden P0 on Windows: storage faults, long downloads, cancellation during publication, and migration tests.
2. Finish the engine interface and bridge contracts before starting the desktop shell.
3. Build the P1 desktop shell and exercise every lifecycle command from the UI.
4. Complete the P1 release gate: installer, upgrades, categories, and update safety.
5. Move to P2 browser capture and batch/CLI once the desktop workflows are stable.
6. Keep P3 and P4 as prioritized backlog items; do not schedule them until their dependencies are met.
