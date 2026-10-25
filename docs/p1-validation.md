# P1 validation status

P1 daily-use features are implemented in source, with the restrictions below.
Localization is excluded at the user's request. The **release gate remains open**
until the native interactive checks pass. Browser capture and batch/CLI follow
this gate; scheduling depends on the completed batch/CLI contract.

## Automated coverage

- `pnpm test`: category precedence, presentation sorting, bulk partial failures,
  unfinished grouping, and description search.
- `pnpm test:ui`: real Chromium flows for deferred entry, properties, category
  filtering, layout customization, onboarding, keyboard focus return, reduced
  motion, scaled layouts, and axe WCAG AA checks on populated lists and settings
  in both themes. These use MockBridge, not native file operations.
- `cargo test --workspace`: deferred admission without network/file creation,
  restart and explicit resume, destination reservation, native camelCase IPC,
  settings persistence, schema-4 migration/older-writer rejection, no-overwrite,
  cancellation, recovery, custom working-directory publication/owned cleanup,
  staging collisions, session-only credentials, and cross-origin auth stripping.
- `pnpm build`, `cargo fmt --all -- --check`, and
  `cargo clippy --workspace --all-targets -- -D warnings`.

### Recorded run — 2026-10-25, Windows x64

| Check | Result |
| --- | --- |
| `pnpm test` | 7 passed |
| `pnpm test:ui` (Chromium) | 5 passed, including both-theme axe checks |
| `cargo test --workspace` | 85 passed including the doctest; 1 large test ignored in the default suite |
| Formatting, Clippy with warnings denied, production web build | Passed |
| NSIS bundle build | Passed; `target/release/bundle/nsis/OSDM_0.1.2-alpha.1_x64-setup.exe`, 5.99 MiB |
| Separate opt-in large streaming test | Passed; 2 GiB + 12,345 bytes, working-set growth below 128 MiB, exact length and hash checks; 1402.90 s test wall time |

The large-test run includes debug-build transfer and subsequent whole-file
verification, not a controlled throughput benchmark. It does not report exact
peak working set, CPU, or disk metrics; the measured memory result is a bound.
The installer is an unreleased local build, not an install/upgrade validation or
a signed release. Interactive checks below remain outstanding.

## Native checks — not yet exercised for this change

Record Windows version, app version, result, and reproduction details locally.
Do not export the database or source URLs.

1. Install the NSIS package on a clean supported Windows installation. Add an
   immediate and deferred download; verify only the immediate one transfers.
2. Edit category rules and appearance, partially download a file, exit via tray,
   then upgrade using `/UPDATE /P /R`. Verify settings, queue order, deferred
   state, partial progress, completed history, and downloaded files survive.
3. Verify update decline, checksum rejection, interrupted download, and version
   downgrade refusal. Exercise the signed path when configured.
4. Exercise folder picker cancel/success, Explorer reveal, open-file handler,
   close-to-tray transfers, second-instance focus, completion notifications, and
   coordinated Quit. Reopen and resume deferred work.
5. Uninstall and verify the documented user-data policy. Reinstall and check
   retained data if applicable. Record cleanup behavior rather than assuming it.
6. Keyboard only: add/inspect/defer, navigate rows, resume/pause, sort every column,
   restore queue order, edit category rules, delete history, and close dialogs.
   Check visible focus and return focus; nested buttons must remain operable.
7. Narrator: check dialog names, field labels, error announcements, state/progress,
   sort direction, and selection. Test both themes at 100%, 150%, and 200% scaling
   and with reduced motion. Measure text/focus contrast; do not infer a pass from
   token names or screenshots alone.

## Implemented scope and safety restrictions

- Per-download description/category metadata, category navigation (including
  deleted categories), category rule editing, automatic routing, and folder overrides.
- Download Later; finished/unfinished views; multi-selection and explicit deletion
  choices; sortable/hideable columns; configurable toolbar visibility/order.
- URL editing for paused/failed jobs. Destination editing for stopped unpublished
  jobs: filename changes retain partial data; folder changes require no existing
  partial/layout. Existing files are never moved or overwritten.
- Basic authentication is explicit, HTTPS-only outside loopback tests, and scoped
  to one transfer/origin. Secrets are never written to SQLite or snapshots and
  must be reentered after restart or source replacement. No cookie export.
- Custom temporary storage is restricted to the destination volume and requires
  hard links. Final publication is still same-directory; cross-volume copying is
  not implemented. Destination storage is the safe default.
- Optional start confirmation/immediate start, progress dialog/minimized panel,
  completion dialog, silent mode, and native completed-file copy drag-out.
- Optional external scanner with literal arguments, exact `{file}` substitution,
  null stdio, kill-on-drop and five-minute timeout; at most eight concurrent
  completion action tasks. Excess actions report an error and retain the file for
  manual scanning. Neither a successful exit nor a computed hash is a malware verdict.
- Custom completion WAV playback/test on Windows (10 MiB limit); silent mode
  suppresses notifications, sound, and completion/progress dialogs, not opted-in scans.
- First-run connection check plus a repeatable Settings check; no default remote
  endpoint or request before the user supplies a URL.
- Strong ETag already takes precedence over modification time. Eligible matching
  ETags allow resume despite Last-Modified changes; mismatched date-only validators
  restart. No unsafe validator-bypass setting is provided.

These restrictions are intentional safety boundaries, not claims of unrestricted
filesystem relocation, cross-volume temporary storage, or universal authentication.
Windows Authenticode signing remains a release-owner decision; no signing identity
or production updater key is fabricated by this implementation.
