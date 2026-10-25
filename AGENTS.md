# AGENTS.md

Guidance for AI coding agents working in this repository. Read the linked
documents when changing their subject matter.

## Project

OSDM is a Windows-first desktop download manager:

- `crates/download-engine/` — headless Rust/Tokio HTTP(S) engine.
- `src-tauri/` — thin Tauri 2 desktop shell and native workflows.
- `src/` — React 19 + TypeScript UI, Zustand store, and bridge seam.
- `docs/` — bridge contract, design notes, and phased roadmap.

The engine owns transfer state, scheduling, retries, persistence, integrity, and
publication. The desktop shell delegates to it. The UI renders engine snapshots
and must not invent progress, connection counts, or integrity results.

## Commands

Run from the repository root:

```powershell
pnpm install
pnpm dev                 # browser UI with mock engine
pnpm desktop:dev         # native desktop app
pnpm build               # typecheck + production web build
cargo test -p download-engine
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Build the Windows installer:

```powershell
pnpm tauri build --bundles nsis
```

The installer is written to `target/release/bundle/nsis/` (or
`src-tauri/target/...` in a non-workspace checkout).

Do not search or edit `target/`, `node_modules/`, `dist/`, or `src-tauri/gen/`.

## Non-negotiable engine invariants

1. Never overwrite an existing destination during an ordinary download. Initial
   publication remains a same-directory hard link with conflict detection.
   Explicitly enabled synchronization may replace an unchanged OSDM-owned file
   at its existing path, but only after verifying ownership, file identity, and
   the last published content. Persist replacement intent first; stage and verify
   the new file separately, retain a recoverable old copy until completion is
   durable, and use a tested atomic replacement protocol. Reject unrelated,
   user-modified, or concurrently changed files. Never stream into or truncate
   the existing destination. See `docs/synchronization-replacement.md`; this
   policy exception does not mean replacement is implemented.
2. Persist intent before destructive filesystem work. Invalidate durable offsets
   before truncating or reusing a temporary file.
3. Checkpoint — flush plus sync — before advancing committed bytes in SQLite.
4. Every long wait must observe the download cancellation token: retries,
   cooldowns, storage waits, throttling, and hashing.
5. Stream bodies with bounded buffers. Do not allocate from an untrusted
   `Content-Length`.
6. Ranged reuse requires a matching validator, length, and range support.
   Incompatible sources restart rather than mixing versions.
7. Source URLs and signed query strings are secrets. Never log them, add them to
   fixtures, or attach the database to an issue.
8. One engine owns a database. Preserve the OS file lock and shutdown contract.
9. Database schema changes need a transactional migration, upgrade coverage, and
   downgrade rejection coverage. Older binaries must not bypass integrity rules.
10. Do not present planned features as shipped. Update
    `docs/intelligent-downloads.md` when scope changes.

## Updates

- Release builds check public GitHub Releases; never install without explicit
  consent or without verifying the matching `SHA256SUMS.txt` entry.
- Windows updates must launch the NSIS installer with `/UPDATE /P /R` so the
  existing installation is upgraded in place instead of uninstalled.
- Keep release asset suffixes compatible with the updater
  (`_x64-setup.exe`, `_amd64.deb`) and publish `SHA256SUMS.txt`.
- The signed-manifest path remains available when `OSDM_UPDATE_ENDPOINT` and
  `OSDM_UPDATE_PUBKEY` are configured at build time.

## Change workflow

- Keep engine protocol/state logic out of `src-tauri/`; commands validate and
  delegate.
- Update both `TauriBridge` and `MockBridge` when the bridge contract changes,
  then update `docs/bridge-contract.md`.
- Add regression coverage for behavior changes, especially failure, restart,
  cancellation, and concurrent access.
- Preserve unrelated user changes. Do not reformat whole files for a focused fix.
- Use concise conventional commit subjects, for example
  `feat(engine): adapt host concurrency`.

## Release workflow

The release workflow is `.github/workflows/release.yml` and runs on `v*` tags,
published GitHub Releases, and manual dispatches with an existing tag. It builds
the Windows NSIS installer and Linux Debian package. Before tagging, keep these
versions identical:

- `package.json`
- `crates/download-engine/Cargo.toml`
- `crates/cli/Cargo.toml`
- `crates/browser-bridge/Cargo.toml`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

Add the release to `CHANGELOG.md`, run the quality gates, then:

```powershell
git tag -a v0.1.0-alpha.1 -m "OSDM v0.1.0-alpha.1"
git push origin master
git push origin v0.1.0-alpha.1
```

The workflow tests, builds both packages, creates or updates the GitHub Release,
and uploads `SHA256SUMS.txt`. Use a manual workflow dispatch with the existing
tag to attach a platform package to an already-published release. Updater signing
keys remain optional until signed updater artifacts are enabled.

## Source of truth

- Architecture and engine behavior: `crates/download-engine/README.md`
- UI/engine contract: `docs/bridge-contract.md`
- Roadmap and acceptance gates: `docs/intelligent-downloads.md`
- Contribution expectations: `CONTRIBUTING.md`
- Release history: `CHANGELOG.md`
