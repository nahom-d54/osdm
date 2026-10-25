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

1. Never overwrite an existing destination file. Publication is a same-directory
   hard link with conflict detection.
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

The release workflow is `.github/workflows/release.yml` and runs on `v*` tags.
Before tagging, keep these versions identical:

- `package.json`
- `crates/download-engine/Cargo.toml`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

Add the release to `CHANGELOG.md`, run the quality gates, then:

```powershell
git tag -a v0.1.0-alpha.1 -m "OSDM v0.1.0-alpha.1"
git push origin master
git push origin v0.1.0-alpha.1
```

The workflow tests, builds the NSIS installer, publishes the GitHub Release, and
uploads `SHA256SUMS.txt`. Updater signing keys remain optional until signed
updater artifacts are enabled.

## Source of truth

- Architecture and engine behavior: `crates/download-engine/README.md`
- UI/engine contract: `docs/bridge-contract.md`
- Roadmap and acceptance gates: `docs/intelligent-downloads.md`
- Contribution expectations: `CONTRIBUTING.md`
- Release history: `CHANGELOG.md`
