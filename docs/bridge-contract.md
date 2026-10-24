# P1 bridge and engine contract — UI side

The P1 desktop shell (PLAN §6.2.1) is implemented against the bridge described
here. Per the working agreement for this effort: anything that touches the
download engine or the Tauri shell is **documented below and assumed done**;
the UI only implements the frontend.

While `src-tauri/` does not yet provide these commands, the app runs in a
browser with `MockBridge` (`src/lib/bridge/mock.ts`), which simulates the
engine contract: probe → segmented transfer with checkpoints → no-overwrite
publication. The UI never special-cases the mock.

## 1. Engine API in use (exists today, P0)

Mirrored 1:1 from `crates/download-engine` (`Engine`, `Config`, `Download`,
`Segment`, `DownloadState`). Serde output is consumed verbatim:

- Rust struct fields keep snake_case (`total_bytes`, `eta_seconds`,
  `temporary_path`, `bytes_per_second`, `ready_to_publish`).
- `DownloadId` is a UUID string; `PathBuf` serializes as a string.
- `DownloadState` variants appear as their Rust names verbatim:
  `Queued | Downloading | Pausing | Paused | Canceling | Canceled | Failed | Completed`.
- `Segment { start, end, committed, downloaded }`; `end: null` only for a
  single streaming transfer.

Engine semantics the UI relies on:

- Enqueue persists before returning; the destination parent must exist and an
  existing destination file is never overwritten (the Add dialog double-checks
  with `path_exists`, but the engine remains the authority).
- Pause/cancel drain workers before returning; cancel keeps temporary data and
  history.
- Resume is a no-op for queued/active/completed; recovered downloads open
  paused; retry revalidates and requeues failed/canceled work.
- `reorder` receives **every** queue/history id exactly once, full list order
  preserved except the requested move.
- Progress events are best-effort full snapshots at up to 4 Hz; after
  subscriber lag the UI calls `snapshot()`.
- Concurrency config is fixed for the engine's lifetime; only the speed limit
  can change at runtime (`set_speed_limit`, `0` = unlimited).
- Public network errors omit URLs; the UI surfaces them as-is in the banner
  and per-download error text.

## 2. Tauri commands the UI calls (assumed available)

Command names and payload keys as invoked in `src/lib/bridge/tauri.ts`.
Results map to the engine's `Result<T, Error>`; rejections surface their
string message to the user.

### Engine lifecycle and commands

| Command | Args | Result | Notes |
| --- | --- | --- | --- |
| `engine_open` | `config: Config` | `()` | Called once at app start with the persisted settings' concurrency values. Must fail loudly if the OS file lock is held (single-instance conflict). |
| `engine_shutdown` | — | `()` | Coordinated shutdown: stop scheduling, drain workers, persist final state, release the lock. |
| `engine_enqueue` | `request: { url, destination }` | `id: string` | Persisted before success, per engine contract. |
| `engine_pause` / `engine_resume` / `engine_retry` / `engine_cancel` | `id: string` | `()` | No-op semantics follow the engine (resume on terminal states does not throw). |
| `engine_reorder` | `ids: string[]` | `()` | Full ordered id list, each id exactly once. |
| `engine_set_speed_limit` | `bytesPerSecond: number` → engine arg `bytes_per_second`? | `()` | See note below. |
| `engine_snapshot` | — | `Download[]` | Full snapshot in queue/history order. |

> Note: `ts` side sends `bytesPerSecond` (camelCase arg). Rust side must
> either accept that arg name or the tauri layer renames it to
> `bytes_per_second`; the command boundary owns the conversion.

### Events

| Event | Payload | Cadence |
| --- | --- | --- |
| `engine://snapshot` | `Download[]` (full snapshot) | up to 4 Hz while anything changes; UI also refreshes with `engine_snapshot` every 5 s |
| `engine://error` | `string` (engine `Error` text, URL-redacted) | on engine-internal failures (persistence/scheduler) |

### Desktop workflows (shell, not engine)

| Command | Args | Result |
| --- | --- | --- |
| `pick_directory` | `title: string, defaultPath?: string` | `string \| null` (native folder dialog) |
| `path_exists` | `path: string` | `boolean` |
| `open_path` | `path: string` | `()` — opens the file with the OS default handler (opener plugin) |
| `reveal_path` | `path: string` | `()` — shows the file in Explorer |

Shell behaviors assumed implemented Rust-side (no UI control flow depends on
them beyond availability):

- **Single instance**: a second launch focuses the existing window or reports
  the file-lock error through `engine_open`.
- **Hidden-window transfer safety**: closing the window hides to tray; app
  exit only via explicit Quit, which awaits `engine_shutdown()` before the
  process ends (PLAN §3.4, §5.1).
- **System tray** and **completion notifications** (notification on
  Completed; never fires for Failed without user-visible error state).
- **Settings persistence**: the UI persists settings to webview
  `localStorage` today; the shell is expected to mirror/own the real store so
  settings survive webview data loss and are written atomically (PLAN §3.4:
  config files never contain secrets).
- **Updater**: manifest verification, downgrade refusal, and consent are
  shell-side per PLAN §6.2.2/§9; the UI has no update controls in alpha.

## 3. Pending engine additions required by P1

These are engine-side gaps the PLAN requires for the P1 gate. The UI calls
them already (mock supports them); each needs an engine implementation with
tests, a migration/reorder-contract check, and documentation:

1. **`engine_remove_download(id, remove_partial_file)`**
   - Removes the entry (and history record) from persistence.
   - `remove_partial_file: true` deletes the temporary file **only if it is
     still the engine's own temporary path**; it must never delete a
     published destination, and must refuse while workers could be writing
     (terminal states only; the UI gates this but the engine enforces it).
   - Interaction with `reorder`'s "every id exactly once" contract must be
     specified so a later reorder after deletion is valid.
   - PLAN hooks: §3.4 (separate deletion command added with the desktop
     release), §6.2.2 (delete selected; partial-file cleanup separate from
     history removal), §8.2 (behavior tests).
   - Status: **pending in engine** (README "Current limits" states no
     deletion/history-removal command exists).
2. **`engine_cleanup_partial_data()`**
   - Deletes leftover temporary files of Failed/Canceled/entryless transfers,
     returning the count removed. The UI exposes this later in Settings or
     the toolbar (not yet wired); the bridge surface exists.
   - Status: **pending in engine** (automatic leftover-part cleanup is listed
     as a current limit).
3. **Optional: `engine_capabilities()`**
   - Reports filesystem support for hard-link publication (NTFS/ext4/APFS)
   - letting the UI warn when the destination directory is on FAT/exFAT,
     before a download is enqueued into it (PLAN §4.8). Not yet called by the
     UI; recommended before the release gate (§6.2.3).

## 4. Sequencing

- UI work proceeds against `MockBridge`; it switches automatically to
  `TauriBridge` when `__TAURI_INTERNALS__` exists (i.e., when `src-tauri/`
  ships the app shell).
- The switch criterion is name-for-name match with the tables above; no UI
  changes beyond `src/lib/bridge/tauri.ts` (plus eventual arg naming) should
  be needed.
- The P1 exit gate milestones that remain after the shell exists: installer/
  upgrade tests, category routing (P1 §6.2.2 not yet in UI), accessibility
  pass (§8.3), and preservation of settings/queue/history across updates.
