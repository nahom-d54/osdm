<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/osdm-as-text.png">
    <img alt="OSDM — adaptive desktop download manager" src="docs/assets/osdm-as-text.png" width="280">
  </picture>
</p>

<p align="center">
  <strong>Measured, resilient downloads for the desktop.</strong><br>
  A Rust download engine and Tauri shell built around safe progress, honest telemetry, and no surprises.
</p>

<p align="center">
  <a href="crates/download-engine/README.md">Engine</a> ·
  <a href="docs/intelligent-downloads.md">Roadmap</a> ·
  <a href="CHANGELOG.md">Changelog</a> ·
  <a href="CONTRIBUTING.md">Contributing</a> ·
  <a href="CODE_OF_CONDUCT.md">Code of Conduct</a> ·
  <a href="LICENSE">MIT License</a>
</p>

<p align="center">
  <img alt="Rust 1.89+" src="https://img.shields.io/badge/Rust-1.89%2B-B7410E?logo=rust&logoColor=white">
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white">
  <img alt="TypeScript 5.9" src="https://img.shields.io/badge/TypeScript-5.9-3178C6?logo=typescript&logoColor=white">
  <img alt="Windows 10/11" src="https://img.shields.io/badge/platform-Windows%2010%2F11-0078D4?logo=windows&logoColor=white">
  <img alt="Status: alpha" src="https://img.shields.io/badge/status-alpha-D9A23E">
  <img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-57C48F">
</p>

---

OSDM downloads ordinary HTTP(S) files with the care of a transfer tool and the restraint of an instrument. It splits large files into durable segments, adapts its connection count to the server it is actually talking to, and keeps committed bytes safe across pauses, failures, and restarts. Finished files are checked for length and, when a SHA-256 reference is available, verified before they are published.

> [!NOTE]
> OSDM is alpha software. The transfer engine is tested, but interfaces and storage revisions may still change between releases. Keep backups of important files and read release notes before upgrading.

## Why OSDM

Most downloads fail loudly for ordinary reasons: a server rate-limits a burst of connections, a connection drops mid-file, a signed URL expires, or a laptop sleeps. OSDM treats those as normal operating conditions.

| Under pressure                         | What OSDM does                                                                                                 |
| -------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| The server returns `429` or `503`      | Honors `Retry-After`, pauses the whole host, and keeps retrying automatically instead of failing the download. |
| Extra connections make things slower   | A per-host controller reduces concurrency, shrinks request ranges, and retries only after a recovery delay.    |
| A connection breaks after saving bytes | Resumes from the last synced checkpoint; partial progress is never thrown away unnecessarily.                  |
| The network is healthy again           | Cautiously tests one additional connection and keeps it only when throughput improves.                         |
| A download link expires                | Lets you replace the URL on a paused or failed download; resuming revalidates saved bytes before reuse.        |
| A file arrives corrupted               | Computes SHA-256 before publication and blocks publication when a supplied checksum does not match.            |

## Features

- **Adaptive connections.** Connection settings are ceilings, not fixed worker counts. Each host starts conservatively and scales against observed throughput, latency, errors, and rate limits.
- **Durable segmented transfers.** Large ranged files are split into independent segments with synced checkpoints. Pause and cancel drain cleanly; recovery opens interrupted work paused.
- **Smart retry and recovery.** Transient failures use jittered backoff. `429`/`503` responses share a host-wide cooldown and do not exhaust the ordinary retry budget. Every wait is cancellable.
- **Integrity before publication.** SHA-256 is computed with bounded memory. Supply the publisher’s checksum when adding a download to get `Verified`; without one, OSDM still reports a `Computed` hash rather than pretending it is proof.
- **Source replacement.** Change the URL of a paused or failed download without losing its destination. The engine reuses checkpoints only when range support, validator, and length still match.
- **Desktop workflow.** Add-and-probe flow, queue ordering, pause/resume/retry, per-download details, speed limiting, completion notifications, and close-to-tray behavior.
- **P1 daily-use workflows (source).** Download Later, descriptions/categories,
  stopped destination editing, multiselect history removal, session-only Basic
  authentication, same-volume temporary storage, configurable toolbar/columns,
  completion/progress dialogs, copy drag-out, and opt-in scanner/WAV actions.
  See [P1 validation and safety limits](docs/p1-validation.md); native release
  validation remains separate. Localization is skipped for this implementation.
- **Automatic GitHub updates.** OSDM checks published GitHub Releases after launch and every six hours, can include prereleases, and downloads only after confirmation. Packages are checked against the release’s `SHA256SUMS.txt`. Windows updates install in place with `/UPDATE`, so the previous installation is not uninstalled.
- **Local-first by default.** No telemetry. The queue lives in a local SQLite database, and the engine is independent of the desktop shell.

The adaptive connection, recovery, and integrity foundation is implemented. Mirror intelligence, per-download priorities, storage reservations, duplicate detection, predictive scheduling, automatic organization, and media detection are planned in [the phased roadmap](docs/intelligent-downloads.md).

## Install

OSDM ships a **Windows 10/11** NSIS installer and a **Linux Debian package** for Ubuntu 24.04 and newer. Open this repository’s **Releases** page and download the matching package plus `SHA256SUMS.txt`.

Release builds are produced automatically from version tags, from published GitHub Releases, or from a manual workflow run. Verify the downloaded asset hash before running it. Builds from source are described below.

Installed builds check for updates through **Settings → Updates** and the tray menu. On Windows, in-app updates run the installer with `/UPDATE`, upgrade the existing installation in place, and restart OSDM. If you run a downloaded setup manually, select **Don’t uninstall** on the upgrade page or launch it with `/UPDATE` to keep the current installation.

## Build from source

### Requirements

| Tool                  | Version                                                                              |
| --------------------- | ------------------------------------------------------------------------------------ |
| Rust                  | 1.89 or newer                                                                        |
| Node.js               | 22 or newer                                                                          |
| pnpm                  | 10                                                                                   |
| Desktop prerequisites | [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform |

### Run the desktop app

```powershell
pnpm install
pnpm desktop:dev
```

The native shell starts the Vite dev server, opens the webview, and connects to the Rust engine.

### Run the browser shell

```powershell
pnpm dev
```

Without Tauri, the UI uses a deterministic mock engine. It is useful for interface work and requires no Rust toolchain.

### Test the transfer engine

```powershell
cargo test -p download-engine
```

The engine suite covers ranged transfers, resumption, checkpoint cancellation, rate-limit recovery, adaptive connection behavior, checksum verification, publication safety, database migration, and Windows path handling.

## Project layout

| Path                      | Responsibility                                                                                             |
| ------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `crates/download-engine/` | Headless Tokio download engine: probing, segment scheduling, retries, persistence, integrity, publication. |
| `src-tauri/`              | Native desktop shell, Tauri commands/events, settings, tray, notifications, and update verification.       |
| `src/`                    | React/TypeScript interface, engine bridge, mock engine, and design system.                                 |
| `docs/`                   | Bridge contract, design notes, and the phased intelligent-download roadmap.                                |
| `.github/workflows/`      | Tag-driven release automation.                                                                             |

The engine has no Tauri or frontend dependency. The UI consumes full snapshots from a narrow bridge; see [the bridge contract](docs/bridge-contract.md).

## Quality gates

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm typecheck
pnpm build
```

Changes to transfer behavior need regression coverage. Bug reports should include the failing state, whether progress was committed, and the relevant HTTP status — never a signed URL containing credentials.

## Releases

The [release workflow](.github/workflows/release.yml) runs when a `v*` tag is pushed, when a GitHub Release is published, or when started manually with an existing tag. It:

1. Verifies the tag matches the version files.
2. Runs the engine test suite.
3. Builds the Windows NSIS installer and the Linux Debian package.
4. Creates or updates the GitHub Release for the tag.
5. Attaches every package and `SHA256SUMS.txt` for verification.

The Linux package targets Ubuntu 24.04 and newer. A tag push is still the fastest path; publishing a GitHub Release or running the workflow manually rebuilds and attaches the missing platform assets.

Keep the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` in sync before tagging. Optional Tauri updater signing keys can be supplied as `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets; updater endpoints and public keys remain build-time configuration.

## Security and privacy

- OSDM sends no telemetry.
- The local database contains full source URLs, including signed query strings. Treat `downloads.db` as a secret and never attach it to an issue.
- Network errors omit request URLs.
- Existing destination files are never overwritten.
- GitHub updates require explicit consent, use HTTPS, and are verified against the release’s `SHA256SUMS.txt` before they can run. Checksums are not a publisher signature; signed-manifest updates are used when a publisher configures them.
- The Windows installer is not code-signed yet, so SmartScreen may warn.

Please report security-sensitive findings privately to the maintainers rather than in a public issue.

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) for setup, quality gates, engine invariants, and review expectations. By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

OSDM is released under the [MIT License](LICENSE).

<p align="center">
  <sub>Built with Rust, Tauri, and an unreasonable respect for checkpoints.</sub>
</p>
