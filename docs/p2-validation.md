# P2 capture and automation validation

P2 has an implementation across the engine, desktop, CLI, and browser targets.
**The full P2 release gate is not passed.** Automated checks and outstanding
interactive checks are distinct. Synchronization currently publishes changed
**revisions**, not in-place replacements. The user has approved a narrow policy
exception for explicitly enabled, verified replacement of unchanged OSDM-owned
files; implementation and validation remain outstanding. See
[the replacement policy](synchronization-replacement.md).

## Available in source

| Area | Behavior |
| --- | --- |
| Desktop entry | Batch URL textarea, explicit clipboard paste, UTF-8 link-file import (2 MiB limit), normalized duplicate suppression, existing folder selection, Start now / Save for later, partial-result reporting. |
| Capture offers | URL drag/drop; optional clipboard offers on window focus for this session only. Capture always opens a review surface; it never silently queues clipboard content. Existing completed-file native drag-out remains available. Clipboard denial is nonfatal. |
| Queues | Named queues; create, rename, delete, assign stopped/terminal entries, sidebar filtering, Start Queue / Stop Queue. Start does not silently retry historical failed/canceled work. Stop prevents new member admissions and drains both transfers and source checks. Deleting a stopped queue removes its schedules/assignments, not files/history. |
| Schedules | Multiple one-time or fixed-interval UTC schedules, enable/disable/delete, optional bounded run window, persisted next slot, skip-missed default, optional run-once-on-startup, overlap suppression. CLI `run-scheduler` keeps a headless owner alive; the desktop must be running or started at sign-in. No wake-from-sleep service is claimed. |
| Synchronization | Opt-in per queue; probes completed sources using engine connection/cooldown budgets. Matching nonempty validator + length skip a new transfer; changes or unverifiable identity download a fresh revision. Old copies remain intact. Checks are cancellable and shutdown drains them. Failed revisions require explicit retry. Removing source history requires first unassigning it from the synchronization queue. |
| Completion | Session-only opt-in Windows shutdown or named RAS/VPN disconnect, after every selected queue file completes. Unrelated runnable work blocks it. New work cancels an already-visible, monotonic 60-second countdown. Final engine quiescence and durable shutdown precede the action. No Wi-Fi/VPN management stack, silent shutdown, or dial-up redial is added. |
| Windows sign-in | Explicit current-user Start with Windows toggle. No administrator elevation. |
| Browser | Separate Chromium/Firefox MV3 targets, explicit link/selected/page capture, opt-in interception, exclusions/file suffixes, optional modifier bypass, native-message allowlists, authenticated IPv4 loopback forwarding, durable prepare/commit/abort leases, per-transfer explicit cookie handoff. Host registrations are explicit scripts, including vendor-specific custom browser executable/key support. |
| Distribution | Release workflow builds separate CLI/native-host capture-tools archives and includes them in SHA256SUMS. A separate manual extension workflow builds checksummed packages and optionally requests unlisted Mozilla signing or Chrome draft upload through protected CI secrets. No signing/publication has been performed. Disposable Windows native-host registration is exercised by the live harness. |

## Time and restart policy

Times are UTC seconds. The UI accepts a local date/time and converts it once to a
UTC instant. Recurrences add an elapsed interval (60 seconds to one year), not a
local-calendar rule: a 24-hour interval can move by one local clock hour across
DST. Changing timezone changes display, not the slot identity. Ambiguous local
times follow the platform date parser; check the saved displayed time. Local-time
calendar/DST-fold selectors are not implemented.

The scheduler persists consumption before dispatch. On startup, missed slots
skip unless run-once-on-startup is enabled; many missed slots coalesce into one.
During normal operation, a slot up to 30 seconds late runs; later slots skip.
Thus sleep/forward-clock jumps do not trigger a catch-up storm. Backward clock
jumps cannot rerun an already-consumed slot. Overlapping slots for a running or
source-checking queue are consumed/skipped, not stacked. Stop windows use their
persisted UTC end; a forward clock jump may end a transfer window sooner. The
power countdown itself uses monotonic time. Recovered transfers remain paused;
an uncompleted consumed run is not automatically replayed after a crash.

## Automated checks

- Engine integration tests: persistent membership/schedules and schema 4→5 upgrade;
  previous-binary rejection; default missed-run skip and startup catch-up;
  backward clock jumps; persisted slot consumption and run-window stopping;
  batch duplicates/conflicts; nonrunnable capture leases, idempotent commit/abort,
  expiry/restart; unchanged source skip and byte-exact changed revision publication.
- Extension tests: distinct Chromium/Firefox-shaped adapters, no-host fallback,
  pause/prepare/cancel/commit ordering, cancellation failures, exclusions, suffixes,
  modifiers, normalization. These are mocked adapters, not live browser installs.
- Native helper: bounded frame parsing and protected descriptor round trip. Native
  capture rejects unauthenticated/wrong-direction envelopes, browser web origins,
  unknown request fields and unsafe filenames. IPC dispatch checks frontend argument names.
- Desktop browser tests: batch admission/duplicates, queue navigation and stop,
  schedule creation, unsafe-input rejection, keyboard close and both-theme axe.

Run `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, `pnpm test`, `pnpm test:extension`, and `pnpm test:ui`.
Record actual command results after execution; this checklist alone is not a pass.

### Recorded source validation — 2026-10-25

- Full Rust workspace tests and doctests: **pass**; the opt-in multi-gigabyte test
  remains ignored. Includes nine automation engine tests, nine CLI integration
  tests, protected/encrypted helper tests, and native mock-runtime IPC/power-policy
  checks. No real power action is executed by the tests.
- Workspace/all-targets Clippy with `-D warnings`: **pass**. Rust formatting: **pass**.
- `pnpm test`: **9 pass**. `pnpm test:extension`: **12 pass**, across both browser-
  shaped adapters. Type checking and production web build: **pass**, with Vite's
  existing bundle-size advisory (no performance improvement is claimed).
- Playwright: **9 pass**, including P1 regressions and both-theme P2 axe/keyboard
  checks. The final run initializes the persisted theme before rendering, avoiding
  an earlier color-transition scan race without disabling contrast assertions.
- PowerShell registration/preparation scripts and workflow YAML: syntax checks
  **pass**. Scripts were not executed against real startup/registry/browser state.
- The first full Rust build ran out of D: disk space. No user files or build
  artifacts were deleted. Final Rust checks used
  `C:\Users\nahom\AppData\Local\Temp\opencode\osdm-p2-target`, two build jobs,
  incremental compilation disabled, and `profile.dev.debug=0` /
  `profile.test.debug=0`; debug assertions and the normal test suite remained on.

These automated results do not close the live validation gaps below.

### Additional live-browser/source validation — 2026-10-25

- `pnpm test:native-browser -Browser both -SkipBuild`: **pass**, using installed
  Chrome 138.0.7204.157 and Firefox 154.0.1 in headless disposable profiles.
  Debug desktop/helper binaries were built with test isolation. Normal profiles
  were not opened; current-user native-host default values were backed up before
  temporary registration and restored during cleanup. No shutdown/disconnect,
  startup registration, signing, upload or public distribution was executed.
- Both browsers: native handshake, durable prepare/abort, explicit commit and
  SHA-256 fixture publication, automatic interception with byte-exact engine
  publication, failed handshake while the app is stopped, reconnect after app
  restart, and missing-helper-executable fallback to byte-exact browser downloads:
  **pass**. Cross-origin redirects received no fixture cookies/Authorization.
  Chrome's persisted extension worker was explicitly woken on reload. Firefox's
  temporary add-on was reinstalled for its second browser launch, **not** tested
  as a persistent signed installation.
- The live run exposed browser pause-state differences; the automatic handoff
  guard now requires the transfer still be paused before canceling it, accepts
  either in-progress or interrupted pause shapes, and aborts if the user resumed
  it. Regression adapters cover both shapes and user resume. The smoke harness
  distinguishes terminal `USER_CANCELED` from a temporary interrupted pause.
- Local evidence (profiles/fixtures retained, not suitable for automatic issue
  upload): `%LOCALAPPDATA%\Temp\opencode\osdm-native-browser-OFDbHy\results.json`.
- `pnpm test:extension`: **17 pass** (14 adapter/rule checks and three packaging/
  draft-upload checks). Chromium/Firefox ZIP builds plus SHA256SUMS: **pass**.
  Firefox `web-ext lint`: **0 errors, 0 notices, 0 warnings** after setting the
  supported Firefox minimum to 142 for no-data-collection manifest metadata.
- Helper/desktop Rust tests and doctests: **pass** (3 helper and 15 desktop
  tests). Workspace/all-targets Clippy with `-D warnings`: **pass** after using
  a direct settings initializer in test setup. Rust formatting, workflow YAML,
  and JavaScript syntax checks: **pass**. Full engine/UI results above were
  recorded earlier; this increment did not rerun those full suites.
- Signing/draft-upload workflow is manual-only, account steps default off, and
  documentation requires environment reviewers and CI secrets. No account
  credentials were supplied or used. Draft upload tests use an injected mock
  transport; they do not establish a real store upload/signing pass.

## Remaining release checks / scope differences

- Extend the disposable real-browser pass above: verify idle service-worker
  suspension, outdated host (missing executable is covered),
  browser update, disabled integration, exclusions, modifier timing, notifications,
  cookie permission denial and fresh credential handoff after restart. Explicit
  fixture headers/redirect stripping are covered, not the interactive cookie UI.
- Inject app/host/browser termination at every handoff step. There is an unavoidable
  uncertain-acknowledgement window after browser cancellation; the extension asks
  for user attention rather than risking a silent duplicate transfer. See the
  [protocol and limits](../extension/README.md). Validate lease cleanup on restart.
- Browser-store review/signing; Firefox temporary extensions do not survive browser
  restart. Current source tests cannot establish the real-browser restart gate.
- Native Windows clipboard permissions, external-app CLI examples, startup registry,
  custom-browser registration, real scheduled stop/overlap, sleep/resume, timezone
  changes and clock adjustments. Do not execute power actions on a user's machine
  during automated checks. Validate shutdown/disconnect on a disposable test system.
- In-place synchronization replacement is **approved but not implemented**.
  The repository now permits explicitly enabled replacement of unchanged
  OSDM-owned files under the verified, atomic, crash-recoverable
  [replacement policy](synchronization-replacement.md). Implement and test that
  protocol before enabling replacement; unrelated files and ordinary downloads
  remain protected. Current synchronization still creates separate revisions.
- Live extension distribution/installer/tool-archive validation and outstanding
  P1 native checks remain. No live release or signing key was created by this work.

These gaps prevent declaring P2 fully complete, even when automated tests pass.
