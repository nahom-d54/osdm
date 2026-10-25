# P2 start decision — CLI foundation

The user requested starting P2 before the remaining interactive P1 release checks
were completed. This is a scoped reprioritization, not a declaration that P1 passed.
Those checks remain tracked in [p1-validation.md](p1-validation.md).

Start §6.3.2's independent batch/CLI foundation first. It exercises existing engine
admission and recovery without adding a browser permissions/credential surface or
changing desktop lifecycle, the bridge, persistence schema, or transfer core.
Browser native messaging versus authenticated loopback remains a design decision;
do not expose an unauthenticated listener as a shortcut.

The first implementation was `crates/cli`, with explicit database ownership,
URL/file-list input, deferred admission, duplicate ID reuse, start-queue, JSON
results, and noninteractive exit codes. See its [contract](../crates/cli/README.md).
Its duplicate policy is script idempotency against retained history, not the
content-aware duplicate detection in the intelligent-download roadmap. Subsequent
work added desktop batch/clipboard/drop surfaces, persistent named queues and UTC
schedules, safe synchronization revisions, Chromium/Firefox extension targets and
the protected native helper, CLI desktop queue triggers, Windows startup, and
session-only opt-in completion actions. Current behavior, limitations and release
gates are tracked in [p2-validation.md](p2-validation.md).

## Gates

- Automated: queue-only survives restart without networking; repeat runs reuse IDs;
  invalid syntax opens no database; per-item conflicts produce a failing exit while
  preserving successful admissions; existing files remain untouched; concurrent
  ownership is refused; resumed/direct downloads produce exact fixture bytes.
- Existing engine and workspace regression gates must remain green. No performance
  claim or transfer tuning is introduced.
- Remaining gates are no longer just CLI work: interactive interrupt validation,
  real browser capture/reconnect/security, store signing, and native installer/
  tool-archive validation remain. Scheduling/queues are now implemented in source
  following the initial persistence/idempotency tests; the full P2 priority gate
  remains open.

## Initial CLI validation (before broader P2 changes)

- `cargo test -p osdm-cli`: seven integration tests pass, including transfer
  failure exits and closed-output-pipe shutdown with retained admission.
- `cargo clippy --workspace --all-targets -- -D warnings`: passes; CLI-only
  Clippy also passes after the output-pipe regression was added.
- `cargo fmt --all -- --check`, `pnpm test` (seven tests), and `pnpm build`: pass.
  Vite reports a bundle-size advisory. Windows Rust builds report nonfatal
  incremental-cache access-denied notes.
- `cargo test --workspace`: the longer retry passes all enabled tests and
  doctests; the opt-in multi-gigabyte test remains ignored. This run included
  six CLI tests; the subsequent CLI-only run passes all seven, including the
  added closed-output-pipe regression. The initial 120-second run timed out.
- Interactive Ctrl+C and P1 native release validation remain outstanding.
