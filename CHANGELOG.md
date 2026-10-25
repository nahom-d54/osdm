# Changelog

All notable changes to OSDM are documented here. This project follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[Unreleased]: https://github.com/nahom-d54/osdm/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/nahom-d54/osdm/releases/tag/v0.1.0-alpha.1
