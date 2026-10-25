# Contributing to OSDM

Thanks for taking the time to improve OSDM. This guide covers the development
workflow, the engine invariants that keep downloads safe, and what a reviewable
change looks like.

By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
For security-sensitive reports, follow the guidance in the [README](README.md#security-and-privacy)
instead of opening a public issue.

## Ways to contribute

- Report reproducible bugs with the state, HTTP status, and whether bytes were
  committed. Never include signed URLs, cookies, or database files.
- Improve tests, documentation, accessibility, and error messages.
- Implement an item from the [intelligent download roadmap](docs/intelligent-downloads.md).
- Review pull requests, especially changes to transfer, persistence, or
  publication behavior.

For a large feature, open an issue describing the behavior, failure modes, and
acceptance checks before writing the implementation. For a small fix, a focused
pull request is enough.

## Set up

### Prerequisites

- Rust 1.89 or newer (the workspace uses the 2024 edition)
- Node.js 22 or newer
- pnpm 10
- Platform prerequisites for [Tauri 2](https://v2.tauri.app/start/prerequisites/)

On Debian/Ubuntu, the Tauri prerequisites look roughly like:

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev librsvg2-dev
```

Install JavaScript dependencies and start the app:

```powershell
pnpm install
pnpm desktop:dev
```

To work on the interface without Rust, run the browser shell:

```powershell
pnpm dev
```

It uses `MockBridge` with deterministic transfer behavior.

## Useful commands

| Command | Purpose |
| --- | --- |
| `pnpm dev` | Vite UI with the mock engine |
| `pnpm desktop:dev` | Native Tauri app and Rust engine |
| `pnpm typecheck` | TypeScript only |
| `pnpm build` | Typecheck and production web build |
| `cargo test -p download-engine` | Fast engine suite |
| `cargo test --workspace` | Engine plus native shell tests |
| `cargo fmt --all -- --check` | Formatting gate |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lint gate |

The optional multi-gigabyte streaming test is ignored by default because it
writes about 2.1 GiB:

```powershell
$env:OSDM_LARGE_TEST = "1"
cargo test -p download-engine --test downloads multi_gigabyte_download_keeps_memory_bounded -- --ignored --nocapture
```

It skips itself when the disk has less than 3 GiB free.

## Project map

- `crates/download-engine/` is the headless engine. It owns probing, durable
  segment state, retries, adaptive host feedback, throttling, hashing, storage,
  and publication.
- `src-tauri/` is the desktop shell. Keep it thin: commands should validate and
  delegate to the engine, not duplicate transfer logic.
- `src/lib/bridge/` is the only UI-to-engine seam. `TauriBridge` maps commands;
  `MockBridge` must preserve the same observable behavior for browser work.
- `src/` is the React interface. Render engine snapshots as authoritative; do not
  invent progress or connection counts.
- `docs/bridge-contract.md` documents commands, events, and serialized fields.
  Update it when the contract changes.
- `docs/intelligent-downloads.md` records phased scope and acceptance gates.
  Do not mark planned work as shipped.

## Engine invariants

These rules are not stylistic preferences. Breaking one risks data loss or
silently false progress.

1. **Never overwrite a destination.** Publication uses a same-directory hard
   link and refuses conflicts.
2. **Persist intent before destructive filesystem work.** Invalidate durable
   offsets before truncating or reusing a temporary file so a crash cannot turn
   preallocated zeroes into claimed progress.
3. **Checkpoint before admitting progress.** Flush and sync file data before
   advancing a committed offset in SQLite. Live bytes are allowed to be re-read
   after recovery; committed bytes are not guesses.
4. **Keep waits cancellable.** Pause, cancel, storage waits, rate-limit cooldowns,
   and hashing must all observe the download's cancellation token.
5. **Bound memory and request sizes.** Stream bodies. Do not allocate based on
   an untrusted `Content-Length`.
6. **Respect validators.** A validator is required for ranged reuse. If range
   support, length, or validator changes, discard partial state rather than mix
   versions.
7. **Treat URLs as secrets.** Do not log source URLs, signed query strings, or
   database contents. Network errors deliberately omit URLs.
8. **One engine owns a database.** The OS file lock is part of the contract.
9. **Make schema changes transactional and tested.** Include upgrade coverage
   and a downgrade rejection test. Older binaries must not silently bypass new
   integrity requirements.
10. **Measure, then claim.** Performance and “intelligent” behavior need tests or
    repeatable measurements. Adaptive limits should have a reason visible to the
    user, not a hidden heuristic.

## Making changes

### Tests

- Add regression coverage for behavior changes and bug fixes.
- Engine tests live in `crates/download-engine/tests/` and in module-level
  `#[cfg(test)]` blocks.
- Debug-only fault injection belongs in `src/test_faults.rs`; it must not be used
  by the desktop application.
- Frontend changes should at least pass `pnpm build`. If behavior depends on the
  bridge contract, update both bridges and the mock.
- Do not weaken an assertion to make an intermittent test pass. Fix the race or
  document the measurement being asserted.

### Rust style

- Run `cargo fmt --all`.
- Keep `cargo clippy --workspace --all-targets -- -D warnings` clean.
- Prefer explicit errors and narrow public APIs. The engine is a library as well
  as the desktop backend.
- Comments should explain ordering constraints, protocol details, or why a
  seemingly simpler approach is unsafe.

### TypeScript and UI style

- `pnpm typecheck` must pass. Avoid `any` and non-null assertions unless the
  invariant is local and obvious.
- Follow the existing component and token conventions in `src/styles.css`.
- Show state from snapshots. Do not infer completion, concurrency, or integrity.
- Keep controls keyboard accessible and do not signal state with color alone.

### Commits

Use a concise, imperative subject. Scopes are useful when they identify the area:

```text
feat(engine): adapt host concurrency from measured throughput
fix(ui): keep retry visible after checksum mismatch
docs: clarify database downgrade behavior
test(engine): cover cancellation during verification
```

Keep unrelated refactors out of behavior changes when practical. If a change
requires a manual database or settings migration, call it out in the pull request.

## Pull requests

A good pull request includes:

- What changed and why.
- The user-visible behavior and any tradeoffs.
- The commands you ran and their results.
- Screenshots or recordings for visual changes.
- Follow-up work that is deliberately out of scope.

Reviewers focus on correctness under failure, cancellation, restart, and
concurrent access before style. Expect changes that affect transfer or
publication to receive deeper scrutiny.

## Releases

Releases are built by [`.github/workflows/release.yml`](.github/workflows/release.yml)
when a `v*` tag is pushed. Before tagging:

1. Update the version in `package.json`, `src-tauri/Cargo.toml`, and
   `src-tauri/tauri.conf.json`. The workflow rejects a tag that does not match
   `tauri.conf.json`.
2. Run the quality gates locally.
3. Confirm the database schema migration is safe for the previous release.
4. Push the tag, for example `git tag v0.1.0 && git push origin v0.1.0`.

Updater signing is optional until updater artifacts are enabled. When it is
enabled, configure `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as repository secrets and never commit the
private key.

## License

By contributing, you agree that your contribution is licensed under the
repository’s [MIT License](LICENSE).
