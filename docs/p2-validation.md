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
| Browser | Separate Chromium/Firefox MV3 targets, explicit link/selected/page capture, opt-in interception using URL or browser filename (including late filenames), exclusions/file suffixes, optional modifier bypass, native-message allowlists, authenticated IPv4 loopback forwarding, durable prepare/commit/abort leases, per-transfer explicit cookie handoff. Guided Windows Chrome/Edge/Brave/Chromium/Firefox setup and installer helper setup require consent, refuse foreign registrations and report session-wide native handshakes without scanning profiles. Opt-in media response-metadata observation can discover HTTP-backed blob/JS/frame sources without reading headers/cookies/bodies. Manual scripts retain vendor-specific custom browser support. |
| Distribution | Desktop build wrapper stages the native helper and allowlisted Chromium/Edge/Firefox files as Tauri resources; no automatic registration or browser installation. Release workflow also builds separate checksummed CLI/native-host archives. Optional real store metadata enables official-listing buttons; none is fabricated by default. Signing/publication stays a separate approved workflow. See [guided setup](browser-setup.md). |

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

### Release-candidate validation — 2026-11-01

- `pnpm test`: **18 pass**; `pnpm test:extension`: **51 pass**, including
  URL-scoped automatic cookies/global opt-out, safe cookie-store selection,
  revoked permissions, EXE filename handoff and frame-bound media selections.
- Production UI build/typecheck: **pass** (existing bundle-size advisory).
  Full Playwright suite: **57 pass**, including React/Vite extension hash routes,
  cookie permission/opt-out UI, floating controls, settings tabs, identity editing
  and both-theme accessibility. Extension UI tests use browser API fixtures; they
  do not establish a real browser permission-dialog pass.
- Latest desktop Rust tests: **29 pass**; workspace/all-targets Clippy with
  `-D warnings` and Rust formatting: **pass**. Full workspace tests passed before
  the latest UI/identity increment; desktop tests cover that increment.
- Debug Windows NSIS installer: **built**, not an interactive installer lifecycle
  pass. Release archives now stage compiled extensions rather than raw React/Vite
  source that cannot be loaded without a Node build.
- Disposable Chrome 138 smoke: **pass** for browser URL/path-scoped synthetic
  cookies, global opt-out and authenticated native prepare/abort; EXE interception
  with byte-exact publication; embedded blob/JS network discovery; reconnect and
  missing-host fallback. Zero credential forwarding leaks were recorded. Existing
  native-host registrations were restored and verified by the harness. Evidence:
  `%LOCALAPPDATA%\Temp\opencode\osdm-native-browser-plHzHa\results.json`.
- Firefox package lint: **0 errors, 0 notices, 2 warnings** in bundled React DOM's
  internal `innerHTML` assignments. Application UI uses escaped React text and
  does not use `dangerouslySetInnerHTML`; actual store review remains pending.
- Automatic cookies apply only to HTTPS ordinary file capture, not media jobs.
  Live permission prompts, signed Firefox restart, authenticated production sites
  and broad playback compatibility remain unverified.

### Default-on capture and confirmation follow-up — 2026-11-01

- New preferences default interception, local integration, cookies, floating
  controls and network discovery on; explicit saved opt-outs remain respected.
  Extension installation/update now requests cookies, webRequest and HTTP(S)
  host access, with static all-frame video-button scripts. Browser permission
  withholding/restricted pages are not bypassed.
- Desktop native commit now uses `commit_capture_paused`, followed by a reviewed
  start/later/discard dialog; closing and restart retain accepted work paused.
  New extension prepares include `review:true` to fail safely against older
  immediate-start desktop builds. The headless engine commit API is unchanged.
- `pnpm test`: **18 pass**; `pnpm test:extension`: **52 pass**; full Playwright:
  **64 pass**. Includes opt-outs, 460px width, review cancellation/failure,
  static/default-on floating controls, shadow-root discovery and accessibility.
- Engine automation tests: **10 pass**, including paused acceptance surviving
  lease expiry/restart and idempotent browser retries. Desktop Rust tests:
  **30 pass**. Workspace/all-targets Clippy with `-D warnings`, formatting,
  typecheck and production web build: **pass**.
- Disposable Chrome 138 smoke: **pass** for review-only accepted handoffs,
  no automatic publication, default static floating control beside a genuinely
  playing synthetic canvas video in a cross-origin frame, source selection,
  HTTP-backed blob/JS manifest discovery, cookie lookup, reconnect and missing-host
  fallback. This is not broad production-player/DRM/authenticated-media coverage.
  Evidence: `%LOCALAPPDATA%\Temp\opencode\osdm-native-browser-Bov9u0\results.json`.
  The first direct Cargo build omitted Tauri's custom-protocol feature and could
  not load the UI without a dev server; the corrected embedded-UI build passed.
- Firefox package lint remains **0 errors, 0 notices, 2 bundled React DOM
  warnings**. No installer, signed Firefox or browser-store release is claimed
  for this follow-up.

### Draggable video panel, captured identity, default user agent — 2026-11-01

- The floating video panel is draggable by pointer or arrow keys with viewport
  clamping, closable per playing video, and lists every discovered source plus an
  optional subtitle. Closing does not disable detection; a different player can
  show it again. UI tests cover drag, close, multi-video choice, stale-source
  rejection and dropdown labels without query secrets.
- Captured file downloads replay the browser's user agent and a bounded allowlist
  of non-secret identity headers (`Accept`, `Accept-Language`, `Sec-CH-UA*`) for
  the original request. Cookies remain the separately approved, URL-scoped lookup.
  Request bodies, response headers, context headers (`Referer`/`Origin`/
  `Sec-Fetch-*`), conditional/range and transport headers are not accepted.
  Captured identity is memory/session-only; cookies/authorization are stripped
  across redirect origins.
- Ordinary downloads and media inspection use an editable browser-like user agent,
  defaulting to the Chrome 156 Windows string. Native settings validate a
  single-line header value (1–512 characters), persist it, and apply changes to
  subsequent requests. The UI saves only a valid draft and can reset the default.
- Automated evidence: `pnpm test:extension` **65 pass**; full Playwright **72
  pass**; `cargo test --locked --workspace` **243 pass/3 ignored**; workspace/all-
  targets Clippy with `-D warnings` and formatting **pass**. Engine tests verify
  configured/captured identity application, invalid replay-header rejection,
  plain-HTTP secret rejection and cookie/authorization stripping across redirects;
  desktop tests cover settings persistence/validation and IPC argument names.
- Disposable Chrome 138 smoke **pass**: automatic synthetic cookie lookup plus a
  native prepare carrying the browser user agent and `Accept-Language`, review-only
  handoffs with no automatic publication, default floating controls in a playing
  cross-origin frame, embedded blob/JS discovery, reconnect and missing-host
  fallback; zero credential leaks. Evidence:
  `%LOCALAPPDATA%\Temp\opencode\osdm-native-browser-0K0fO5\results.json`.
  The smoke does not prove every production player, CDN header policy, DRM, or
  authenticated media job. The request-header observer and privacy behavior still
  need broader real-site review before claiming universal capture.

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
