# Intelligent download delivery plan

The agreed scope is **all features, delivered in phases**. Each phase includes
engine behavior, desktop controls/status, persistence/recovery, and regression
tests. A feature is not considered implemented merely because it has a setting.

## Phase 1 — reliable adaptive transfers

Implemented in source; validation is tracked with the phase's test results.

| Feature | Priority | Behavior |
| --- | --- | --- |
| Adaptive multithreading | Critical | Start with at most two host connections. Sample throughput every two seconds, test one extra connection after healthy windows, and withdraw it if it does not improve throughput. Respect global, host, and per-file ceilings, including resumed layouts. |
| Smart retry and recovery | Critical | Resume failed ranged requests from synced offsets. Retry 429/503 without exhausting the transient-error budget, honor Retry-After, share host cooldowns, and keep pause/cancel responsive. Replacement URLs revalidate partial data. |
| Network-aware downloading | High | Reduce host concurrency on rate limits, transient errors, latency spikes, and repeated throughput degradation/stalls. Retire excess ranged requests after checkpointing. Adapt HTTP range request sizes between 256 KiB and 8 MiB and increase the retry floor while recovering. |
| Automatic integrity verification | Critical | Compute SHA-256 with bounded memory before publication. Compare against an optional publisher/user checksum and prevent publication on mismatch. Retry a mismatched partial from scratch. Distinguish a computed hash from verified integrity. |

The feedback controller is host-wide, so several downloads do not independently
overload the same server. Per-file settings still cap each download. Existing
non-resumable streams finish rather than being repeatedly discarded to rebalance.
User bandwidth limiting suspends throughput-based tuning so deliberate throttling
is not mistaken for network degradation; actual request errors still reduce load.
The inspector exposes current connections, effective host limit, request size,
and the controller's decision. Durable segments are separate from live connections.

Checksums are optional because many publishers provide no reference. A computed
SHA-256 alone is **not** proof of integrity or authenticity. Existing downloads
without a reference remain usable, and old history is not marked verified.
Database revision 3 introduced downgrade protection for checksum requirements;
current revision 4 also protects metadata and temporary-storage ownership.

### Acceptance gate

- A server that starts rejecting parallel requests causes live request counts to
  fall, with no checkpoint loss and no retry storm.
- Healthy sustained throughput allows cautious growth; useless growth is reversed.
- Faults in one host do not lower another host's budget.
- Admission, cooldown, transfer, and hashing remain cancellable.
- Exact final bytes survive subrequest boundaries, pause/resume, and rebalancing.
- Matching checksums publish; mismatches do not. Retry rereads mismatched data.
- Existing records migrate with safe defaults; older binaries reject revision 3.

## Phase 2 — resource and source management

Planned, not implemented in phase 1.

| Feature | Priority | Deliverable and acceptance gate |
| --- | --- | --- |
| Dynamic bandwidth allocation | High | Persist per-download priority; weighted fair token allocation under one global budget. Add a user-selectable low-interference mode with reserved headroom. Priority changes take effect live; low-priority work must not starve. Do not claim browsing/game detection without an actual supported signal. |
| Storage-aware downloading | High | Check available volume space before allocation; reserve remaining bytes across concurrent downloads on the same volume. Recheck unknown-length streams as they grow. Preserve partial data and show the required/free space when the budget is exhausted. Handle sparse/preallocated files and external disk consumers. |
| Duplicate detection | High | Flag same-source/size/validator candidates before transfer and use completed SHA-256 hashes for exact matches. Never treat a matching name or size alone as identity. Offer reuse/skip/keep-both without silently deleting data. |
| Mirror intelligence | High | Accept an explicit list of user-authorized sources. Benchmark bounded samples, rank by response time/throughput/error history, and fail over to compatible mirrors. Reuse bytes only with a trusted common checksum or sufficient validated identity; preserve per-host cooldowns and budgets. Surface the selected mirror and reason. |

Sequence: space reservations and priority accounting first, then duplicate index,
then mirror admission/ranking. Mirror requests must participate in all existing
connection, storage, integrity, and bandwidth budgets.

## Phase 3 — automation

Delivery order: finish the standalone P1 release gate in `PLAN.md`, then browser
capture and batch/CLI. Scheduling depends on the batch/CLI persistence and
automation contract. Category destination suggestions are available in the add
dialog, with durable category assignments and category navigation; these do not
constitute intelligent organization or scheduling. Localization is excluded from
the current P1 implementation at the user's request. See `p1-validation.md` for
implemented daily-use workflows and outstanding interactive release checks.

Planned, not implemented in phase 1.

| Feature | Priority | Deliverable and acceptance gate |
| --- | --- | --- |
| Predictive download scheduling | Medium | Maintain local time-of-day throughput history with sample confidence. Schedule nonurgent transfers in better windows, with a deadline and Run now override. Sparse data, clock changes, offline periods, and app restarts must not strand a job. |
| Intelligent file organization | Medium | Use MIME type, safe extensions, and user-defined project/category rules to propose/create folders. Allow per-download overrides and conflict handling. Never move existing files implicitly or infer project membership from arbitrary untrusted path components. |
| Smart media detection | Medium | Identify direct downloadable audio/video using MIME and content metadata; detect supported manifest/stream formats and expose available choices. Explicitly distinguish downloadable sources from recognized-but-unsupported formats. Add protocol-specific downloading only with bounded parsing, cancellation, and integrity/recovery coverage. |

The scheduling and organization features are opt-in. Learning stays local and
must not record credentials or signed URLs as analytics keys. Media and mirror
selection use sources the user has supplied or authorized.

## Validation ownership

Luna owns regression tests and frontend/native checks for these milestones.
Phase completion reports list actual checks and any remaining gaps; planned
features are never presented as shipped.
