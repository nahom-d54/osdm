# P1 bridge and engine contract — available implementation

The native bridge is implemented in `src-tauri/src/lib.rs`. The frontend selects
`TauriBridge` inside Tauri and `MockBridge` in an ordinary browser. The Rust
download engine remains independent of Tauri.

**Status:** the commands, event forwarding, settings store, tray lifecycle, and
engine additions below are available in source. Publisher updates are implemented
but configuration-gated; this repository does not contain a production signing
key or update endpoint. Native desktop release validation is listed in §5.

## 1. Engine data and semantics — available

Serde output is consumed verbatim:

- Struct fields use snake_case (`total_bytes`, `eta_seconds`, `temporary_path`,
  `bytes_per_second`, `ready_to_publish`).
- `DownloadId` is a UUID string; paths serialize as strings.
- States are `Queued | Downloading | Pausing | Paused | Canceling | Canceled |
  Failed | Completed`.
- `Segment { start, end, committed, downloaded }`; `end: null` denotes a single
  streaming transfer whose length was unknown.

Contracts:

- Enqueue persists before success. Parents must exist. Destinations are never
  implicitly overwritten; `path_exists` is only a UI preflight.
- Pause/cancel drain workers. Cancel retains data and history.
- Resume is a no-op for queued, active, and completed work; failed/canceled work
  can be requeued. Recovery opens interrupted work paused.
- Reorder requires **every currently retained history/queue ID exactly once**.
  After removal, omit the removed ID. Other entries keep their relative order.
- Concurrency is fixed until the engine is reopened; bandwidth can change live.
- Network error strings omit request URLs. Local databases contain source URLs
  and signed query strings and must not be uploaded as diagnostics.

## 2. Tauri commands — available

Command names match `src/lib/bridge/tauri.ts`. The outer command arguments use
camelCase; nested Rust structs retain snake_case. Errors reject with a string.

| Command | Arguments | Result |
| --- | --- | --- |
| `engine_open` | `config: Config` | `()` |
| `engine_shutdown` | — | `()` |
| `engine_enqueue` | `request: { url, destination }` | UUID string |
| `engine_probe` | `url: string` | `ProbeInfo` |
| `engine_pause`, `engine_resume`, `engine_retry`, `engine_cancel` | `id: string` | `()` |
| `engine_reorder` | `ids: string[]` | `()` |
| `engine_set_speed_limit` | `bytesPerSecond: number` | `()` |
| `engine_snapshot` | — | `Download[]` |
| `engine_remove_download` | `id: string, removePartialFile: boolean` | `()` |
| `engine_cleanup_partial_data` | — | number of files removed |
| `pick_directory` | `title: string, defaultPath?: string` | path or `null` |
| `default_download_directory` | — | absolute existing folder |
| `path_exists` | `path: string` | boolean |
| `open_path` | `path: string` | `()` |
| `reveal_path` | `path: string` | `()` |
| `settings_load` | — | `AppSettings` or `null` |
| `settings_save` | `settings: AppSettings` | `()` |

`engine_open` is idempotent inside one process (including StrictMode and webview
reloads). Persisted native settings take precedence over the supplied config;
config applies only when opening a new engine. A separate process holding the
engine database lock causes an error. Startup errors are shown in the UI banner.

Paths supplied to filesystem commands must be absolute. Open/reveal resolve an
existing filesystem path and use the OS handler/Explorer through the opener
plugin. Native dialogs return `null` on cancellation.
`default_download_directory` returns the OS download folder when it exists and
falls back to the user home directory, so the add-download dialog never starts
from a placeholder path that does not exist.

### URL probe — available

`engine_probe` runs the transfer's one-byte ranged request without creating a
queue entry. It follows redirects and returns:

| Field | Type | Meaning |
| --- | --- | --- |
| `final_url` | string | URL after redirects; used for the link-derived fallback name |
| `filename` | string or null | `Content-Disposition` filename (RFC 5987 extended values decoded, path components removed) |
| `content_type` | string or null | MIME type before any `; charset=...` parameter |
| `total_bytes` | number or null | Total length when the server reports one |
| `ranged` | boolean | Whether the server honored the one-byte range request |

The probe does not reserve a destination, write a file, or alter history. The
desktop workflow sanitizes `filename` for the target filesystem and prefers it
over the URL path. If the probe rejects, the UI keeps the link-derived name and
lets the user explicitly continue; the engine's enqueue and transfer still
perform their own validation and retries.

### Events — available

| Event | Payload | Behavior |
| --- | --- | --- |
| `engine://snapshot` | `Download[]` | Full snapshots, changed snapshots forwarded at up to 4 Hz |
| `engine://error` | string | Persistence/scheduler/desktop failures |

On Rust broadcast lag the shell obtains a fresh engine snapshot. The frontend
also refreshes every five seconds. Unsubscribe remains safe when called before
asynchronous listener registration finishes.

## 3. Native shell behavior — available

- **Single instance:** second launches focus/show the existing window. The
  database's OS lock additionally protects against other engine owners.
- **Close to tray:** closing the main window hides it. Transfers remain owned by
  Rust. The tray offers Open OSDM, Check for updates, and Quit.
- **Coordinated Quit:** shutdown stops scheduling, drains workers, persists the
  queue, and releases ownership before process exit. Shutdown failures keep the
  app running and show an error. New engine commands are refused during Quit.
- **Notifications:** new transitions to Completed notify once per process.
  Historical completed entries do not notify on startup; failed downloads never
  produce a success notification. OS notification permissions still apply.
- **Settings:** `settings.db` lives in Tauri's per-user application-data
  directory alongside `downloads.db`. A typed, versioned SQLite store commits
  settings atomically with FULL synchronous mode. Only documented non-secret
  settings fields are accepted. Native settings restore before engine startup
  and survive localStorage loss. On first native launch, existing frontend
  settings seed the store. Subsequent changes are mirrored through the bridge.
  Concurrency changes apply on restart; speed changes use the live command.

### Signed updates — implemented, publisher configuration required

Updates are requested explicitly from the tray and require native confirmation
before downloading/installing. There are no background installs.

Build with:

- `OSDM_UPDATE_ENDPOINT`: HTTPS URL of a standard Tauri JSON update manifest,
  without query, fragment, credentials, or template substitutions.
- `OSDM_UPDATE_PUBKEY`: the Tauri/base64-encoded Minisign public-key file.

Publish a detached, textual Minisign signature of the **exact manifest bytes** at
`<endpoint>.minisig`, signed by the same publisher key as the update artifact.
The shell verifies this signature, rejects non-newer versions, and compares the
updater plugin's independently fetched metadata with the signed document before
proceeding. The plugin also verifies the artifact signature before installation.
The queue is shut down before the installer runs. Downloads/settings are stored
outside the installed application directory.

Without endpoint/key configuration, Check for updates reports that updates are
unconfigured. Production publishing, signing-key custody, installer upgrade tests,
and live update delivery are **not claimed available merely because the updater
code exists**. See `src-tauri/README.md`.

## 4. P1 engine additions — available

### `Engine::remove_download(id, remove_partial_file)`

- Only Failed, Canceled, and Completed entries with **no running workers** may
  be removed. Queued, Paused, Downloading, Pausing, and Canceling are rejected.
- `true` unlinks only the expected `.osdm-<id>.part` path in the destination
  directory. Mismatched paths, symlinks, and non-regular files are rejected.
  Published destination names are never removed, even when both names identify
  the same file after interrupted hard-link publication.
- `false` retains the partial and atomically transfers its ownership record to
  an orphan ledger while deleting history. The ledger survives restart.
- Remaining ordering is preserved and persisted positions are compacted, so
  later enqueue/reorder/reopen operations remain deterministic.
- Filesystem deletion precedes history deletion when `true`: if the later DB
  operation fails, history can remain although the requested partial was removed.
  The error is returned; retry removal is safe when the partial is already absent.

### `Engine::cleanup_partial_data()`

- Explicitly removes partials of fully stopped Failed/Canceled entries and
  entryless files recorded in the orphan ledger; returns the count actually
  removed. Repeated cleanup returns zero when nothing remains.
- Never scans arbitrary directories or deletes an unrecorded `.part` file.
- Does not touch queued/paused/active work or published destination names.
- Invalidates recovery/publication progress before deleting retained partials.
  These entries remain in history and can restart safely.
- On an error, earlier successful deletions remain applied; the command rejects
  with the failure and can be retried.

### Persistence migration

Engine schema **2** adds `orphaned_partials`. Revision 0/1 databases migrate in
one transaction without changing serialized Download records or queue order.
Newer schema revisions are rejected. Revision-1 application binaries reject
revision 2 rather than silently discarding orphan ownership; downgrade is not
supported. Back up application data before manually reverting binaries.

### Optional capability preflight

`engine_capabilities` remains **not implemented**, and the UI does not call it.
Hard-link publication still requires NTFS/ext4/APFS or another supporting
filesystem; actual filesystem errors remain authoritative. No unsupported
capability result is fabricated.

## 5. Verification and remaining release gates

Automated coverage includes real Tauri mock-runtime IPC dispatch using frontend
payload names, startup/shutdown and locking, native settings reopen/validation,
history removal, cleanup ownership, no-overwrite behavior, queue reordering, and
revision-1 migration. Run:

```text
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Native interactive checks remain distinct from mock-runtime tests: native folder
dialogs, Explorer/open handlers, close-to-tray and Quit, second-launch focus,
notification permissions, signed production updates, and installer
install/upgrade/uninstall must be exercised on supported Windows installations.
Category routing and accessibility validation remain separate P1 release work.
