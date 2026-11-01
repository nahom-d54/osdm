# P3 — media capture and Site Grabber

P3 was started at the user's request before the remaining P1/P2 native, browser
and release validation gates closed. That is a scoped reprioritization: earlier
priorities are not declared complete here, and their gates remain tracked in
[P1 validation](p1-validation.md) and [P2 validation](p2-validation.md).

This page records implemented P3 behavior, the exact supported subset, the
acceptance evidence, and what is still **not** implemented. A green test suite
for a bounded subset is not evidence for every bullet in `PLAN.md` §6.4.

## Implemented in source

The supported-format implementation includes the media review/capture workflow,
verified merge/publication and completed-input cleanup, plus the Site Grabber
wizard/templates, selective or confirmed automatic retrieval, saved-project
controls, scheduling, caching and filter continuation. The format exclusions below
are explicit; source completion is not native/browser release validation.

### Media pipeline

- `crates/media-capture` (offline, no network; parsers never write files):
  - bounded HLS master inspection (declared bandwidth, resolution, codecs,
    average bandwidth and frame rate) and complete unencrypted VOD plans with
    media sequence;
  - bounded conservative static-DASH `SegmentList` inspection preserving
    adaptation/representation identity, initialization, timescale and duration;
  - validating SRT/WebVTT parsing;
  - cancellable, local-only container-aware remux through an explicitly
    configured FFmpeg executable (MPEG-TS or fragmented MP4 tracks into MP4 or
    Matroska, optional subtitle attachment).
- Engine-owned network inspection:
  - `Engine::inspect_hls` and `Engine::inspect_dash` share global/host
    connection permits, bandwidth throttling, bounded interactive retries and
    host cooldowns with ordinary downloads;
  - every redirect is validated **before** the next request; other origins need
    explicit canonical approval; HTTPS downgrade, loops and long chains fail;
  - requests are unauthenticated — no captured cookies, authorization, Basic
    credentials or referrer;
  - bodies are capped whether or not `Content-Length` is present, with a
    bounded overall deadline; cancellation and shutdown release reads, waits and
    permits, and idle-only shutdown refuses an active inspection.
- Durable media jobs (`Engine::create_media_job`, `list_media_jobs`,
  `pause/resume/cancel_media_job`):
  - an explicit quality or DASH representation must be selected; a master
    variant is never auto-chosen and no DASH track is inferred;
  - child segment downloads use the ordinary engine connection/rate/retry/
    checkpoint path and are persisted before their identity is linked; HLS uses
    MPEG-TS segments, DASH uses an initialization segment plus fragmented-MP4
    segments per selected track;
  - restart reopens jobs paused, and interrupted children are adopted rather
    than duplicated;
  - merging uses the shared local-only remux, then an independent decode, a
    synchronized SHA-256 of the exact output, and recoverable no-overwrite
    publication by same-directory hard link;
  - optional selected SRT/WebVTT is fetched boundedly from an approved origin,
    re-validated, and embedded.
- Desktop workflow: **Save media** accepts a direct file, HLS stream or DASH
  manifest; it shows manifest-declared quality (never measured or invented),
  requires explicit origin approval, exposes output folder/name, description and
  category, lists saved jobs with engine-authoritative progress, and offers
  Pause/Resume/Cancel. Direct MP4/M4V/MOV sources can attach explicitly selected
  subtitles through the same verified remux pipeline. Merged output supports MP4
  or Matroska; unsupported direct remux formats fail explicitly.
- Completed media jobs have an explicitly confirmed cleanup action. Durable
  removal intent blocks resume; verified owned inputs, segment history and the
  recorded merge anchor are removed individually. Modified/foreign files are
  refused, the published output is never unlinked, and interrupted cleanup is
  retryable after restart. A tiny workspace ownership marker remains.
- External HLS VOD renditions: master inspection preserves audio/subtitle group
  associations, language/name/default/forced labels and stable selection keys.
  The UI requires an explicit associated audio choice (including deliberate
  video-only omission); manifest defaults never select a track. Saving retains
  master context and revalidates variant/group identity before fetching tracks.
  Playlist and segment origins require approval; no credentials are forwarded.
  Selected TS video/audio streams are mapped explicitly, preserving their relative
  33-bit MPEG presentation clocks. Segmented WebVTT timestamp maps are normalized
  onto the primary video clock and duplicate boundary cues are removed. Audio/video
  children and selected captions survive pause/restart without losing selection.
- Browser workflow: an explicit popup scan of the active page, a single chosen
  media item (optional player subtitle), persisted global/exact-site disable, and
  an authenticated native `offer_media` handoff. Nothing downloads automatically,
  no cookies are sent, and no media URLs are stored as analytics.
- Optional network discovery observes actual response URLs/content types after
  an explicit `webRequest`/site-permission grant. It can discover HTTP-backed blob
  and JavaScript players, extensionless manifests and embedded-frame media without
  injecting into the frame. Normal scans also inspect all browser-permitted frames,
  keeping each player's subtitles frame-local. No page functions are patched or
  JavaScript variables evaluated; no request headers, cookies, response bodies or
  decryption keys are read. The session-only cache is bounded to 8 tabs × 32 sources,
  offers expire after 5 minutes, and navigation/disable clears cached offers.
- A user-enabled floating button beside a playing video opens explicit source
  selection. Site permission gates injection; one-use 60-second tab/frame-bound
  tokens gate handoff. It neither patches playback nor accepts arbitrary URLs from
  page messages. Popup pages are now React/Vite, hash-routed and keyboard accessible.

### Site Grabber

- `crates/site-grabber`: network-free, serde-persisted, bounded explorer.
  - exact origin and seed directory prefix; robots policy fetched first and
    enforced (including a leading-UTF-8-BOM document marker);
  - redirect targets are validated before any follow-up request; cross-origin,
    credential and HTTPS-downgrade escapes are refused;
  - canonical deduplication (queries retained, fragments stripped), finite
    page/file/depth/byte budgets, extension and path filters, and a bounded
    persisted URL/state allowance so every admitted project remains restorable;
  - discovered files are review-only with unknown size until an engine HEAD
    confirms an exact length; unknown lengths cannot be selected.
- Engine integration (`Engine::create_site_project`, `list_site_projects`,
  `control_site_project`, `queue_site_resources`):
  - validated durable projects, pause/resume/cancel/delete, restart reopens
    paused with the frontier and in-flight intent replayed, and shutdown drains
    site workers before releasing database ownership;
  - all discovery traffic uses engine admission, bandwidth, cooldown, retry,
    cancellation and shutdown policy;
  - queued resources reserve their exact payload and admit an ordinary download
    that persists `max_bytes`/`deny_redirects` **before** dispatch, refuses
    credentials, destination or source rewrites, skips synchronization, and
    requires the exact final length before checkpoint, integrity and publication.
- Desktop workflow: four-step **Site Grabber** wizard (source → scope → filters
  and budgets → review resources) with saved-project open/resume/cancel/delete
  and explicit review before queueing. Deleting a project keeps downloaded files
  and download entries.
- Saved-project policy supports explicitly confirmed automatic matching downloads,
  per-project concurrency (1–32, subordinate to global/host ceilings), UTC one-shot
  or periodic exploration, and opt-in coalesced startup catch-up. Schedule slots
  are consumed durably before dispatch; overlaps skip rather than stack. Pause or
  Cancel disables scheduling. The app must be running; no OS wake task is installed.
- Rechecks and confirmed filter edits retain previous downloads and lifetime byte
  charges. New revisions use distinct filenames and revision-specific review IDs;
  they never replace existing files. Filter edits preserve origin/path scope, save
  paused, and disable scheduling and automatic downloads pending fresh consent.
- Optional conditional HTML caching retains at most 16 pages/512 KiB, always
  revalidates with ETag, excludes private/no-store responses, and never caches
  robots or resource bodies or falls back to stale content on errors. Resource
  descriptions come from bounded plain link/player text.
- Scope-free Documents, Images, Audio and Website resources templates seed finite
  filters/budgets. They retain no source, destination, credentials or consent and
  never enable scheduling or automatic downloads.

## Supported subset and explicit limits

- HLS capture is **unencrypted MPEG-TS VOD merged into MP4 or Matroska**, including
  one explicitly selected external MPEG-TS audio rendition and one segmented
  WebVTT subtitle rendition from the chosen variant's groups.
  Live/event playlists, encryption/DRM, byte ranges, initialization maps,
  discontinuities, raw ADTS/fMP4 audio and in-band closed-caption groups remain
  unsupported. Declared audio/video durations must agree within one second; track
  start clocks must agree within 60 seconds and be discoverable in a 1 MiB TS prefix.
  HLS versions through 6 are accepted only for the supported tags, not all version-6
  features. Subtitle bodies share a 1 MiB aggregate cap and 4,096-cue ceiling;
  styles, regions, cue settings and unsupported timeline semantics fail explicitly.
- DASH capture supports **one static period with explicit `SegmentList`** and
  fragmented-MP4 tracks, with an explicit video and/or audio representation. Live,
  DRM, `SegmentTemplate`/`SegmentTimeline`, multiple periods and text tracks other
  than a single static VTT/SRT segment are rejected.
- Exposed single-file DASH subtitle tracks and browser/player tracks are
  discoverable but never automatically selected. Extensionless endpoints require
  a supported subtitle MIME type; filename/MIME conflicts fail before remux.
  HLS WebVTT playlists support `X-TIMESTAMP-MAP` (including 33-bit wrap); absent
  maps use the specified zero-clock mapping, not a guessed per-segment offset.
- No site adapters or headless browser, no forms/login, no execution of arbitrary
  page JavaScript and no automatic site-wide capture. Network discovery needs a
  real HTTP(S) backing request: wholly local/generated blobs, DRM decryption and
  browser-restricted pages/requests remain unsupported. Discovering a URL does not
  make an unsupported HLS/DASH representation or authenticated endpoint supported.
- Automatic mirroring saves matching known-size files as separate revisions; it
  does not rewrite HTML links into an offline website or replace unchanged files.
  Page/file/depth ceilings apply per exploration; byte accounting spans revisions.
- Cleanup covers fully stopped completed media jobs. Failed/canceled jobs retain
  their inputs for recovery, and unrecorded stale merge attempts are not globbed
  or deleted. Completed cleanup retains a tiny ownership marker.
- FFmpeg is a trusted, explicitly configured local executable, not OS-sandboxed.
  At most two engine media-processing stages run concurrently (also bounded by
  global active-download configuration). Remux has a one-hour deadline, a sampled
  output-size guard and a per-allocation cap; these are not hard OS memory/CPU/disk
  quotas. Explicit completed-job cleanup removes its recorded merge artifact.
- Job and merged-file SHA-256 is locally computed, not publisher-authenticated.

## Acceptance evidence

The subsequent browser-discovery pass records 43 extension/packaging tests and a
real Chrome 138 disposable-profile smoke: an extensionless HLS response requested
by an embedded blob/JavaScript player appears as a selectable offer. The same run
verifies extensionless EXE handoff, byte-exact publication, reconnect and missing-
host fallback, with original registrations restored/verified. This is HTTP-source
discovery evidence, not local-only blob export, DRM support, production-site
playback or a claim that every streaming format can be downloaded.

Run from the repository root (Windows paths shown for the optional FFmpeg steps):

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo test -p osdm-media-capture --test remux -- --ignored   # with OSDM_TEST_FFMPEG/OSDM_TEST_FFPROBE
pnpm typecheck
pnpm test
pnpm test:extension
pnpm build
pnpm exec playwright test
```

Recorded 2026-11-01 on Windows with the locally installed
`C:/apps/ffmpeg/bin/{ffmpeg,ffprobe}.exe`:

- `cargo test --workspace`: **228 passed, 3 ignored** (one opt-in multi-gigabyte
  transfer and two opt-in remux tests; both remux tests also passed explicitly).
- Opt-in remux suite (`--test remux -- --ignored`): **2 passed** using real
  FFmpeg/ffprobe — generated MPEG-TS to Matroska with subtitle extraction and
  language metadata, and fragmented MP4 with separate A/V plus WebVTT.
- Media integration tests: real FFmpeg remux of a completed loopback HLS VOD, and
  a real FFmpeg-generated static DASH asset set downloaded through the engine and
  merged from explicitly selected `SegmentList` tracks; both verify exact output
  hash, racing-output preservation and no-overwrite publication.
- Expanded media integration suite: **14 passed** with installed FFmpeg, including
  direct MP4 plus WebVTT remux, exact-output hash and ffprobe subtitle verification.
  The engine unit suite also records **34 passed**, including a forged-artifact
  cleanup regression that proves the published destination is never unlinked.
- Site integration tests cover robots → HTML → metadata → review → hard-capped
  download, changed length, existing destination, real page/robots caps,
  pause/restart replay, shutdown drain, unconfirmed no-network creation,
  cross-origin/credential redirect refusal, unknown-length refusal and a resumed
  selected payload with a matching validator.
- `cargo clippy --workspace --all-targets -- -D warnings`: **pass**.
- `cargo fmt --all -- --check` and `git diff --check`: **pass**.
- `pnpm typecheck`: **pass**; `pnpm test`: **16 passed**;
  `pnpm test:extension`: **25 passed**; `pnpm build`: **pass** (existing
  large-chunk advisory only); `pnpm exec playwright test --workers=1 --timeout=60000`:
  **41 passed** including
  the new media/site focus, mobile-width, fake-IPC and light/dark axe checks.
- Completed-input cleanup coverage includes unconfirmed removal, modified input
  and foreign-file refusal, restart with pending intent, interrupted partial
  cleanup retry, and published-output preservation. UI tests cover explicit
  cleanup consent, extensionless exposed subtitle selection, and template scope.
- External-HLS focused evidence: five offline tests cover groups, unsafe references,
  malformed declarations/maps, clock wrap, deduplicated captions and bounded TS
  PTS parsing. Three engine integration tests cover explicit/stale group choices,
  unapproved origins, shutdown while fetching captions, and a real FFmpeg VOD with
  offset external audio plus segmented WebVTT after pause/restart. ffprobe verifies
  exactly one selected audio stream, its relative start offset, and zero-origin
  video; subtitle extraction verifies timing/deduplication, and output SHA-256 is
  independently checked. Fake-IPC UI coverage verifies explicit choices and master
  context in the saved request. The expanded Rust workspace and explicit remux
  suites pass. The first full UI run had two unrelated P1/P2 timeouts during heavy
  compilation (new rendition coverage passed); the final single-worker rerun
  completed with **41 passed** and no failures.
- Nine Site Grabber automation integration tests cover durable/coalesced UTC
  decisions, startup behavior, command-only ownership, explicit automatic
  admission, concurrency ceilings, filter continuation, separate revisions,
  lifetime accounting, conditional/no-store caching and schema-8 migration rollback.

### Not verified

- No live real-browser capture run in this pass, no store signing or publication.
- No manual media-playback review of a production stream.
- No OS-level resource sandbox around FFmpeg.
- P1/P2 live release gates remain open; P4 remains planned.

## History

- 2026-10-26: first slice — offline HLS parsing only (8 tests), no engine,
  bridge, schema or publication changes.
- 2026-10-31: engine-owned bounded HLS manifest inspection with explicit
  redirect-origin authorization (103 package tests and 130 workspace tests).
- 2026-11-01: media plans/remux/subtitles, durable HLS and static-DASH media jobs
  with verified publication, browser and desktop workflows, and the resumable
  budgeted Site Grabber (see evidence above).
