# P3 start decision — bounded media inspection

At the user's request, start P3 before the remaining P1/P2 native, browser and
release validation gates close. This is a scoped reprioritization, not a claim
that those priorities or P3 are complete. Earlier gates remain tracked in
[P1 validation](p1-validation.md) and [P2 validation](p2-validation.md).

## First slice

`crates/media-capture` implements offline, bounded inspection of a conservative
HLS subset: master quality choices and complete unencrypted VOD segment lists.
It resolves HTTP(S) references without fetching them, redacts debug URLs and
rejects semantics it cannot preserve. This establishes testable input handling
before adding browser detection, networking, persistence or media merging.

The first slice made no engine changes. Engine-owned network inspection is now
implemented as described below; there are still no desktop bridge, extension
permission, database schema or publication changes. No new transfer path bypasses
engine budgets. No performance improvement is claimed. Existing priority
regressions must remain green.

## Engine-owned inspection

`Engine::inspect_hls` fetches one explicitly supplied manifest and passes its
bounded bytes to the media parser. It shares global/host connection permits,
bandwidth throttling, bounded interactive retries and host cooldowns with ordinary
downloads. An independent no-auto-redirect client validates every redirect before
sending and reacquires the destination host's permits. Other redirect origins
require an explicit canonical-origin allowlist; HTTPS downgrades, redirect loops
and chains longer than ten hops are rejected. All requests are unauthenticated:
no captured cookies, authorization, Basic credentials or referrer are inherited.

Bodies are capped at 1 MiB whether or not Content-Length is present; encoded and
partial responses are rejected. A 60-second overall deadline also bounds slow
responses, retries and cooldowns. Caller cancellation releases reads, waits and
permits; shutdown cancels and drains inspections before releasing database
ownership. Idle-only shutdown refuses an active inspection. Neither inspection
nor the parser fetches variant/segment references, creates a durable media job,
writes output or authorizes a manifest-supplied CDN.

## Gates and remaining work

- Initial slice: exact quality metadata, ordered segment discovery, bounded input,
  URL validation, redacted diagnostics, malformed input and unsupported-feature
  rejection. Run `cargo test -p osdm-media-capture`.
- Implemented next slice: engine-owned bounded/cancellable manifest fetches with
  explicit redirect-origin authorization and no credentials. Regression coverage
  lives in `crates/download-engine/tests/media.rs`; run
  `cargo test -p download-engine --test media`.
- Next: browser offers, desktop inspection/explicit quality selection, and an
  explicit credential policy if authenticated inspection is added. A CDN reference
  in a manifest is not by itself permission to forward credentials.
- Before streaming downloads ship: durable media job/segment identity, restart
  and cancellation coverage, container-aware merging, exact output verification,
  and no-overwrite publication using engine safety contracts.
- DASH, external audio/subtitles, live streams, site adapters and Site Grabber
  are not implemented. Site Grabber needs its own scope/budget/robots policy
  decision before crawling code is introduced.

The full §6.4 quality, subtitle, merge-integrity and bounded resumable mirror
gates remain open. Parsing a playlist does not satisfy any streaming capture gate.

## Recorded validation — 2026-10-26

- Media crate tests: **8 passed**, covering quality metadata, segment order,
  malformed/mixed documents, unsupported semantics, numeric validation, URL
  safety/redaction, and size/entry limits. Doctests pass.
- Media crate all-targets Clippy with `-D warnings`: **pass**.
- Workspace formatting and `git diff --check`: **pass**.
- Workspace tests and workspace Clippy: **not completed**; both timed out after
  240 seconds waiting for the shared Cargo build lock. An unrelated Cargo process
  was holding the build directory; it was left untouched. Media crate checks
  were rerun successfully using an isolated temporary target directory.
- No native/browser/manual media playback or merge verification was performed;
  there is no user-facing capture feature in this slice.

## Recorded validation — 2026-10-31

- `cargo test -p download-engine -p osdm-media-capture`: **103 tests and one
  doctest passed**; the opt-in multi-gigabyte stress test remains ignored.
- `cargo test --workspace`: **130 tests and one doctest passed**, with the same
  opt-in stress test ignored.
- New coverage: **12 media integration tests and 2 unit tests**, including final
  URL resolution, explicit cross-origin redirects, TLS downgrade rejection,
  redirect loops/hop limits, declared/chunked body limits, encoded/partial/invalid
  documents, URL redaction, retries, connection admission, bandwidth throttling,
  cancellation, dropped callers, overall admission deadlines, shutdown draining
  and database reopening.
- `cargo clippy -p download-engine -p osdm-media-capture --all-targets -- -D warnings`:
  **pass**.
- `cargo clippy --workspace --all-targets -- -D warnings`: **pass**.
- `pnpm build`: **pass** (Vite reports the existing large-chunk advisory).
- `cargo fmt --all -- --check` and `git diff --check`: **pass**.
- An earlier concurrent engine/media test invocation exceeded its 240-second
  command deadline while waiting/building; the subsequent complete run above
  passed. These results do not constitute native/browser, media playback,
  merge-integrity or Site Grabber validation. No such completion is claimed.
