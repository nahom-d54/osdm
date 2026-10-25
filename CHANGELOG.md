# Changelog

All notable changes to OSDM are documented here. This project follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
