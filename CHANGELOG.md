# Changelog

All notable changes to OSDM are documented here. This project follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
  Browser capture, desktop batch input, and scheduling remain planned; this is
  not completion of the P2 gate. See `crates/cli/README.md` for safety limits.
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

## [Unreleased]

### Planned

- Dynamic bandwidth allocation with per-download priorities.
- Storage-aware reservations and duplicate detection.
- Mirror benchmarking and failover.
- Predictive scheduling, automatic file organization, and media detection.

See [the phased roadmap](docs/intelligent-downloads.md) for acceptance gates.

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

[Unreleased]: https://github.com/nahom-d54/osdm/compare/v0.1.2-alpha.1...HEAD
[0.1.2-alpha.1]: https://github.com/nahom-d54/osdm/releases/tag/v0.1.2-alpha.1
[0.1.1-alpha.1]: https://github.com/nahom-d54/osdm/releases/tag/v0.1.1-alpha.1
[0.1.0-alpha.1]: https://github.com/nahom-d54/osdm/releases/tag/v0.1.0-alpha.1
