# OSDM CLI — P2 automation contract

Headless batch entry using `download-engine`; no second transfer implementation
or prompts. Queues and schedules use engine schema 5. Build with
`cargo build -p osdm-cli --release`. The binary is `osdm-cli` (`.exe` on Windows).

## Examples (PowerShell)

```powershell
# Persist without probing or starting any network transfer.
osdm-cli add --database C:\Downloads\automation.db --directory C:\Downloads --queue-only https://example.com/archive.zip

# UTF-8 file: one URL per line, blank lines ignored; an initial BOM is accepted.
osdm-cli add --database C:\Downloads\automation.db --directory C:\Downloads --input links.txt --queue-only --silent
if ($LASTEXITCODE -ne 0) { throw 'Batch was not fully accepted; inspect JSON results' }

# Resume all paused/queued entries and wait for completion.
osdm-cli start-queue --database C:\Downloads\automation.db

# Download immediately, choosing a different filename.
osdm-cli add --database C:\Downloads\automation.db --directory C:\Downloads --filename archive.zip https://example.com/file

# Create/edit queues or schedules with a strict engine command JSON file.
# Example command.json: {"action":"save_queue","id":"<UUID>","name":"Night shift"}
osdm-cli configure --database C:\Downloads\automation.db --command command.json
osdm-cli queues --database C:\Downloads\automation.db
osdm-cli add --database C:\Downloads\automation.db --directory C:\Downloads --queue '<UUID>' --queue-only --input links.txt
osdm-cli start-queue --database C:\Downloads\automation.db --queue '<UUID>'
osdm-cli stop-queue --database C:\Downloads\automation.db --queue '<UUID>'

# Keep a headless owner alive for saved schedules; Ctrl+C saves and drains.
osdm-cli run-scheduler --database C:\Downloads\automation.db

# Trigger a saved named queue in an already-running desktop app.
# Enable local browser integration in OSDM first; no second engine is opened.
osdm-cli start-queue --desktop --queue '<UUID>'
osdm-cli stop-queue --desktop --queue '<UUID>'
osdm-cli queues --desktop
```

Use `cargo run -p osdm-cli -- <arguments>` when running from source.

## Ownership and safety

- `--database` is mandatory unless `--desktop` is used. Its parent and destination directory must exist.
  Relative paths resolve against the working directory. The CLI uses engine
  defaults, not desktop settings/categories/credentials.
- Only one process owns a database. Quit the desktop app before selecting its
  database, or use a separate automation database. `--desktop` supports only
  listing and starting/stopping saved named queues through the authenticated
  local bridge, not arbitrary filesystem destinations. It cannot be combined
  with `--database` and does not open a database or expose the bridge encryption key.
- `add` validates the entire input syntax before opening the engine, then admits
  each request independently in input order. Admission failures do not roll back
  previously persisted requests. New requests are persisted paused before any
  are resumed. `--queue-only` never resumes work, including existing duplicates.
- Without `--queue-only`, `add` resumes only its accepted IDs and waits for their
  terminal states. `start-queue` selects all paused/queued entries at invocation;
  it does not retry historical failed/canceled entries.
- `start-queue --queue` waits for the selected named queue, including source checks
  and new synchronization revisions. A stopped/incomplete or failed queue exits 1.
  `--desktop` returns after durable dispatch, **not** transfer completion. `queues`
  returns policy/IDs only, no URLs. `run-scheduler` remains alive until Ctrl+C;
  it does not blindly resume recovered paused downloads outside schedule policy.
- `configure --command FILE` consumes the strict `AutomationCommand` JSON shape
  documented in `docs/bridge-contract.md` §7. It supports queue assignments,
  multiple UTC schedules, and optional source-revision checks. Schedules run only
  while a desktop/CLI engine owner remains alive. A private database command exits
  after shutdown; use `run-scheduler` for unattended scheduling.
- Destination names come from the decoded final URL path component, not a probe
  or Content-Disposition. Unsafe/empty names are rejected. `--filename` supports
  exactly one input URL. URL credentials are rejected; fragments are discarded.
- Duplicate identity is **normalized URL + canonical parent and filename** in
  retained engine history, with ASCII case-insensitive destination matching on
  Windows. Repeated input returns the original ID, even for a completed entry;
  it does not prove content identity, check a completed file still exists, or
  reuse files across different URLs. Removing history removes this idempotency.
  Different URLs targeting the same reserved/existing file fail, never overwrite.
- Ctrl+C during the transfer wait drains workers and persists progress before
  exit. Interrupted work can be resumed with `start-queue`. Forced termination
  relies on the engine's crash recovery. HTTP 429/503 may wait indefinitely per
  engine policy; there is no overall CLI timeout yet.
- Source URLs remain sensitive local database data. Output omits URLs, paths,
  credentials, and underlying engine error strings. Do not upload databases or
  link-list files. Command-line URLs may be visible in shell history/process
  listings; prefer a protected input file for sensitive links.

## Output and exit codes

Normal stdout is JSON Lines (one object per line); stderr has no routine output.
`--silent` is accepted for scripts; output is already machine-readable and has no
progress chatter. `--help` alone prints human-readable usage. Additive JSON fields
may appear; scripts should ignore unknown fields. IDs are engine UUID strings.

| `type` | Fields | Meaning |
| --- | --- | --- |
| `accepted` | `index`, `id`, `duplicate` | Request is durable; index is zero-based across URLs and input-file lines in argument order. Not a transfer-success claim. |
| `finished` | `id`, `state` | `Completed`, `Failed`, or `Canceled`; emitted once per selected ID. |
| `error` | `code`, optional `index`, optional `message` | Safe machine-readable failure. |
| `automation` | `state` | Engine queue/schedule snapshot or a durable desktop queue-dispatch result. |

Codes: `invalid_input`, `engine_open_failed`, `enqueue_failed`, `resume_failed`,
`shutdown_failed`, `output_failed`, `automation_failed`, `desktop_unavailable`,
`interrupted`. A closed/broken output pipe
causes coordinated shutdown and exit 1 (the error cannot necessarily be printed).
Detailed native errors are deliberately omitted
to avoid exposing sensitive input. All opened engines receive coordinated shutdown
on ordinary success/error paths. A shutdown failure overrides other exit codes.

| Exit | Meaning |
| --- | --- |
| 0 | All requested admissions succeeded (`--queue-only`), or all selected transfers completed. An empty `start-queue` also succeeds. |
| 1 | Admission, engine ownership/open, resume, transfer, or shutdown failure. Some requests may have succeeded; inspect individual records. |
| 2 | Invalid arguments/input; no database was opened. |
| 130 | Ctrl+C during the transfer wait; progress saved unless shutdown failed. |

## Status and remaining P2 gate

The engine/desktop now implement named queues, UTC schedules, safe source-revision
checks, browser capture transport and session-only completion actions. CLI requests
never arm OS shutdown/disconnect themselves. Native Ctrl+C, browser registration,
power actions, and release archives still need interactive platform validation.
The full P2 gate is tracked in `docs/p2-validation.md`; do not infer completion
from the presence of these commands. Regression tests: `cargo test -p osdm-cli`.
