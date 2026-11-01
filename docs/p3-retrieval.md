# P3 — media capture and Site Grabber

P3 was started at the user's request before the remaining P1/P2 native, browser
and release validation gates closed. That is a scoped reprioritization: earlier
priorities are not declared complete here, and their gates remain tracked in
[P1 validation](p1-validation.md) and [P2 validation](p2-validation.md).

This page records implemented P3 behavior, the exact supported subset, the
acceptance evidence, and what is still **not** implemented. A green test suite
for a bounded subset is not evidence for every bullet in `PLAN.md` §6.4.

## Implemented in source

### Media pipeline

- `crates/media-capture` (offline, no network, no filesystem writes):
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
    playlist is never auto-chosen and no DASH track is inferred;
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
  Pause/Resume/Cancel. Direct-file subtitle attachment is refused with an
  explanation instead of silently degrading.
- Browser workflow: an explicit popup scan of the active page, a single chosen
  media item (optional player subtitle), persisted global/exact-site disable, and
  an authenticated native `offer_media` handoff. Nothing downloads automatically,
  no cookies are sent, and no media URLs are stored as analytics.

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

## Supported subset and explicit limits

- HLS capture is **standalone unencrypted MPEG-TS VOD merged into Matroska**.
  Live/event playlists, encryption/DRM, byte ranges, initialization maps,
  discontinuities and external audio/subtitle renditions are rejected.
- DASH capture supports **one static period with explicit `SegmentList`** and
  fragmented-MP4 tracks, with an explicit video and/or audio representation. Live,
  DRM, `SegmentTemplate`/`SegmentTimeline`, multiple periods and text tracks other
  than a single static VTT/SRT segment are rejected.
- Subtitles are an explicitly chosen SRT/WebVTT source; there is no automatic
  rendition discovery.
- No site adapters or headless browser, no forms/login, no JavaScript-heavy
  capture, no automatic site-wide capture.
- Site Grabber has no scheduled/periodic exploration, no response cache and no
  filter-editing continuation of an existing project; those remain open.
- Media jobs have no removal/cleanup API; workspaces and artifact identity anchors
  are retained.
- FFmpeg is a trusted, explicitly configured local executable. Its output and CPU
  are not OS-sandboxed, and merge artifacts are retained rather than deleted by a
  cleanup command.
- Job and merged-file SHA-256 is locally computed, not publisher-authenticated.

## Acceptance evidence

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

- `cargo test --workspace`: **206 passed, 3 ignored** (the opt-in multi-gigabyte
  transfer test and two opt-in codec tests).
- Opt-in remux suite (`--test remux -- --ignored`): **2 passed** using real
  FFmpeg/ffprobe — generated MPEG-TS to Matroska with subtitle extraction and
  language metadata, and fragmented MP4 with separate A/V plus WebVTT.
- Media integration tests: real FFmpeg remux of a completed loopback HLS VOD, and
  a real FFmpeg-generated static DASH asset set downloaded through the engine and
  merged from explicitly selected `SegmentList` tracks; both verify exact output
  hash, racing-output preservation and no-overwrite publication.
- Site integration tests cover robots → HTML → metadata → review → hard-capped
  download, changed length, existing destination, real page/robots caps,
  pause/restart replay, shutdown drain, unconfirmed no-network creation,
  cross-origin/credential redirect refusal, unknown-length refusal and a resumed
  selected payload with a matching validator.
- `cargo clippy --workspace --all-targets -- -D warnings`: **pass**.
- `cargo fmt --all -- --check` and `git diff --check`: **pass**.
- `pnpm typecheck`: **pass**; `pnpm test`: **14 passed**;
  `pnpm test:extension`: **25 passed**; `pnpm build`: **pass** (existing
  large-chunk advisory only); `pnpm exec playwright test`: **32 passed** including
  the new media/site focus, mobile-width, fake-IPC and light/dark axe checks.

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
