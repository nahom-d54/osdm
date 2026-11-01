# Changelog

All notable changes to OSDM are documented here. This project follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Planned

- Dynamic bandwidth allocation with per-download priorities.
- Storage-aware reservations and duplicate detection.
- Mirror benchmarking and failover.
- Predictive scheduling and automatic file organization.
- Live/DRM capture and HLS fMP4/raw-ADTS renditions.

## [0.1.4-alpha.2] - 2026-11-01

Browser capture and setup fixes. Store publication remains deferred: extension
files still need browser-approved manual loading. Reload updated extension files;
updating the desktop alone does not update an unpacked extension.

### Added

- React/Vite extension UI with hash-routed Capture, Video, Rules and Connect
  pages, OSDM typography, light/dark themes and a permission-gated floating video
  control with explicit source selection and desktop review.
- Automatic URL-matching login cookies for HTTPS file captures, enabled by default
  with a global opt-out and a browser permission grant when enabling capture.
  Values remain session-only and origin-scoped; unsafe cookie contexts stay in
  the browser. There is no per-site preapproval list.
- Tabbed desktop Settings, a consented add/edit/remove extension-ID list per
  browser, expanded capture suffixes, Images/Fonts categories and an explicit
  action to add new built-in category types without replacing customized folders.
- OSDM logo icons for browser toolbars, extension management and the capture popup.
- Opt-in browser response-metadata discovery for HTTP-backed blob/JavaScript
  players and embedded frames, all-permitted-frame scanning, session-only bounded
  offers and explicit site permissions. Local-only blobs/DRM remain unsupported.
- Consented Windows installer helper setup for Chrome, Edge, Brave, Chromium and
  Firefox, ownership-checked silent upgrade repair and uninstall cleanup, without
  starting the engine or enabling capture.
- Stable public-key identities for packaged unpacked extensions, automatic helper
  ID configuration, selected-browser store launch and an advanced-only manual
  fallback and an opt-in store-ready metadata check. Store publication is deferred;
  actual publisher submission/approval remains external and is not claimed here.

### Fixed

- Automatic file capture recognizes browser-reported/late EXE filenames for
  extensionless endpoints; Chrome's filename callback waits for the handoff.
  The popup explains disabled interception and recent handoff failures.
- Native-browser test cleanup now decodes saved registrations correctly on
  Windows PowerShell 5.1 and verifies restored values. Previously it could report
  success while leaving disposable test hosts behind, blocking guided setup.
- Reconnect recognizes owned registrations after installation moves, migrates
  legacy script registrations, and preserves other authorized profile IDs.

## [0.1.4-alpha.1] - 2026-11-01

Differentiated retrieval and guided browser setup preview. Back up application
data before upgrading: schema 10 cannot be reopened by older binaries. Native
installer/browser-store and production playback release gates remain open; live
and DRM media remain unsupported. Extension installation requires browser approval.

### Added

- Guided Chrome/Edge/Firefox setup from onboarding, Settings and Capture & queues:
  explicit per-user helper registration/removal, foreign-registration refusal,
  official-store actions only when configured, manual-file fallback and authenticated
  session-wide connection status. No silent browser-extension installation.
- `pnpm desktop:build` packages the native messaging helper and allowlisted browser
  assets; release builds use that wrapper. Browser setup still requires consent.

- Engine-owned bounded HLS and static-DASH manifest inspection with explicit
  redirect-origin authorization, shared transfer budgets, cancellation and
  shutdown draining.
- Durable media jobs for explicitly selected unencrypted MPEG-TS HLS VOD and
  static fragmented-MP4 DASH `SegmentList` tracks: ordinary checkpointed segment
  transfers, local FFmpeg remux, independent decode, synchronized SHA-256
  verification and recoverable no-overwrite publication, with optional validated
  SRT/WebVTT attachment and category/description metadata.
- Offline bounded media planning: HLS averages/frame rate and VOD sequence,
  conservative DASH `SegmentList` plans, and SRT/WebVTT validation.
- Desktop **Save media** workflow with declared-quality selection, DASH
  inspection, explicit origin approval, and engine-authoritative job controls.
- Browser media panel: explicit active-page scan, one chosen offer with optional
  player subtitle, global/exact-site disable, and authenticated `offer_media`
  handoff without cookies or automatic capture.
- Site Grabber: robots-respecting, same-origin, path-scoped exploration with
  page/file/depth/byte budgets, resumable persisted projects, review-before-
  download, and hard-capped child transfers (`max_bytes`, no redirects, no
  credentials, exact final length).
- Engine schema 7 adds durable media-job and Site Grabber records and rejects
  older binaries rather than bypassing media publication or site guards.
- Confirmed Site Grabber automation: durable UTC one-shot/periodic rechecks,
  explicit startup catch-up, per-project download concurrency, filter continuation
  and bounded ETag-revalidated HTML caching. Revisions preserve previous files and
  lifetime byte charges; schema 8 prevents older binaries from ignoring policy.
- MP4/Matroska output selection and direct MP4/M4V/MOV subtitle remux, plus bounded
  media-processing admission, remux deadline and sampled output-size guards.
- Scope-free bounded Site Grabber templates and exposed extensionless subtitle
  endpoints validated by MIME and content rather than inferred from filenames.
- Confirmed completed-media input/history cleanup with durable retryable removal
  intent (schema 9), modified/foreign-file refusal and published-output preservation.
- External HLS VOD MPEG-TS audio and segmented WebVTT renditions with explicit
  variant/group selection, approved origins, transport-clock alignment, caption
  boundary deduplication and durable pause/restart (schema 10).

P3's bounded subset is implemented in source and tested; it is not claimed
complete against every `PLAN.md` §6.4 bullet. See
[P3 scope and limits](docs/p3-retrieval.md).

See [the phased roadmap](docs/intelligent-downloads.md) for acceptance gates.

## [0.1.3-alpha.1] - 2026-10-25

Capture and automation preview; **not completion of the P2 release gate**.
Synchronization still publishes separate revisions, not in-place replacements.
Extensions are unsigned source targets; store distribution and remaining live
validation are outstanding. Back up application data before upgrading: schema 5
databases cannot be reopened by older binaries. See `docs/p2-validation.md`.

### Added

- P2 desktop batch/clipboard/drop capture, named queue management/navigation,
  persisted UTC transfer windows, missed-run/overlap policies, and safe changed-
  source revision downloads without replacing existing copies.
- Chromium/Firefox extension targets with explicit/automatic capture rules,
  bounded authenticated native-message forwarding, DPAPI-protected Windows
  endpoint discovery, expiring durable handoff leases, and memory-only explicit
  per-transfer cookie/authorization inputs. Real-browser/store gates remain open.
- Saved-queue CLI desktop triggers, a headless scheduler mode, current-user
  Windows startup, and session-only opt-in Windows shutdown/named-RAS disconnect
  with a visible, cancellable countdown and final idle-only engine shutdown.
- P2 UI/accessibility, extension-adapter, lease, schedule and synchronization
  regressions; separate capture-tools release archives. See `docs/p2-validation.md`.
- Disposable real Chrome/Firefox native-capture tests with debug-only app/profile
  isolation, temporary registration backup/restore, restart and fallback checks.
- Manual extension packaging/checksum workflow with optional unlisted Mozilla
  signing and Chrome draft-only uploads via CI secrets; no automatic publication.
- Initial P2 headless CLI with batch URL/file-list input, durable queue-only
  admission, repeat-run duplicate ID reuse, start-queue, and JSON Lines results.
  See `crates/cli/README.md` for safety limits.
- P1 category destination suggestions with built-in and custom categories,
  ordered domain/extension rules, folder overrides, and persistent preferences.
- Optional last-used folder remembering and category-routing regression tests.
- Durable Download Later, sortable download columns, and clear-completed-history
  action that preserves published files.
- Persistent download descriptions/category assignments, category navigation,
  stopped destination editing, unfinished grouping, and multi-select deletion.
- Per-transfer, origin-scoped, session-only HTTP Basic authentication.
- Configurable same-volume temporary storage with safe publication staging and cleanup.
- Completion/progress dialogs, minimized progress, silent mode, immediate-start
  preference, native completed-file copy drag-out, and first-run connection checks.
- Opt-in external scanner with literal arguments/timeouts, Windows WAV completion
  sounds and testing, and persistent toolbar/column customization.
- Chromium workflow, keyboard, scaled-layout, and axe accessibility tests for both themes.

### Changed

- Approved a synchronization-only exception to the no-overwrite policy for
  explicitly enabled, verified, atomic, crash-recoverable replacement of unchanged
  OSDM-owned files. Ordinary downloads remain no-clobber; replacement behavior is
  not implemented by this documentation change.
- Engine schema 5 transactionally upgrades revisions 0–4 and rejects downgrade
  rather than allowing older binaries to bypass queues, schedules or handoff intent.
- Engine schema 4 transactionally upgrades existing databases and prevents older
  writers from discarding metadata or custom temporary-storage ownership.
- Localization is excluded from the current P1 scope at the user's request.

### Fixed

- Bulk actions now report partial failures and reconcile the list after errors.
- Keyboard navigation preserves the selected row's tab stop and leaves nested
  action-button arrow keys alone. Unknown-length progress no longer shows a
  fabricated percentage fill.

## [0.1.2-alpha.1] - 2026-10-25

### Added

- Automatic update checks against public GitHub Releases after launch and every
  six hours, with optional prerelease inclusion and a manual **Check now** action.
- Consent-based update downloads verified against the release’s `SHA256SUMS.txt`
  and GitHub asset digest.
- In-place Windows updates using `/UPDATE /P /R`; the previous installation is no
  longer uninstalled before upgrading.

### Changed

- The release workflow now also runs when a GitHub Release is published and can
  be dispatched manually for an existing tag.
- Releases now include the Linux Debian package alongside the Windows installer,
  with one `SHA256SUMS.txt` covering every downloadable asset.

## [0.1.1-alpha.1] - 2026-10-25

### Fixed

- Removed the WebView browser context menu from production builds, including
  Reload, Save as, Print, and Inspect, and blocked the corresponding common
  browser shortcuts.
- Fixed the Linux CI job by restricting Windows-only large-transfer test helpers
  to Windows and using the platform-local URL for IPC tests.
- Fixed Windows desktop test binaries failing before the harness loaded because
  they lacked the Common-Controls v6 application manifest.

## [0.1.0-alpha.1] - 2026-10-25

First public alpha. Distributed as a Windows NSIS installer.

### Added

- Adaptive host concurrency that starts conservatively, scales against measured
  throughput, and reduces connections on rate limits, errors, latency spikes, and
  stalls without discarding committed progress.
- Network-aware ranged subrequests with adaptive request sizes and a recovery
  retry floor.
- Durable segmented transfers with synced checkpoints, pause/resume, restart
  recovery, and atomic no-overwrite publication.
- Automatic recovery from HTTP `429`/`503`, including full `Retry-After` waits,
  host-wide cooldowns, and retries that do not exhaust the ordinary error budget.
- Source-URL replacement for paused and failed downloads, with revalidation of
  saved bytes before reuse.
- Optional SHA-256 verification before publication, with computed, verified, and
  mismatch states. Mismatched files are never published.
- Desktop queue controls, connection and integrity inspection, speed limiting,
  add-with-probe workflow, completion notifications, and close-to-tray behavior.
- Windows NSIS packaging with automatic tag-driven GitHub Releases and
  `SHA256SUMS.txt` artifacts.
- Engine, bridge, contribution, design, roadmap, and agent documentation.

### Security

- No telemetry.
- Existing destination files are never overwritten.
- Network errors omit request URLs; the local database may contain signed URLs
  and must be treated as secret.
- Database schema revision 3 prevents older binaries from ignoring checksum
  requirements.

### Known limitations

- The installer is not code-signed.
- Windows is the only packaged release target.
- Authentication headers, cookies, proxies, and media extraction are not
  implemented.
- Mirror selection, priorities, storage reservations, duplicate detection,
  scheduling, automatic organization, and media detection remain planned.
- SHA-256 verification requires a trusted reference; a computed hash alone does
  not establish authenticity.

[Unreleased]: https://github.com/nahom-d54/osdm/compare/v0.1.3-alpha.1...HEAD
[0.1.3-alpha.1]: https://github.com/nahom-d54/osdm/releases/tag/v0.1.3-alpha.1
[0.1.2-alpha.1]: https://github.com/nahom-d54/osdm/releases/tag/v0.1.2-alpha.1
[0.1.1-alpha.1]: https://github.com/nahom-d54/osdm/releases/tag/v0.1.1-alpha.1
[0.1.0-alpha.1]: https://github.com/nahom-d54/osdm/releases/tag/v0.1.0-alpha.1
