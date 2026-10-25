# download-engine

A headless Tokio-based HTTP/HTTPS downloader. No Tauri or frontend dependencies.

## Example

```rust,no_run
use download_engine::{Config, DownloadRequest, DownloadState, Engine};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = Engine::open("downloads.db", Config::default()).await?;
    let id = engine.enqueue(DownloadRequest {
        url: "https://example.com/file.zip".into(),
        destination: std::env::current_dir()?.join("file.zip"),
        expected_sha256: None,
    }).await?;

    loop {
        let downloads = engine.snapshot().await;
        let download = downloads.iter().find(|d| d.id == id).unwrap();
        if matches!(download.state, DownloadState::Completed | DownloadState::Failed) {
            println!("{:?}: {:?}", download.state, download.error);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    engine.shutdown().await?;
    Ok(())
}
```

## Behavior

- Segments files above `split_threshold` when range support and a resource validator are available.
- Connection settings are ceilings. A shared host controller starts with at most two connections, samples throughput, cautiously tests additional connections, and scales back on rate limits, errors, latency spikes, stalls, or unhelpful extra concurrency. Excess ranged requests checkpoint and yield; non-resumable streams are allowed to finish.
- Ranged workers fetch adaptive subrequests (256 KiB–8 MiB) within durable segments. Congestion shrinks requests and raises the retry floor. User speed limits suspend throughput tuning, not error-driven backoff.
- Uses a strong ETag when available, otherwise Last-Modified. Missing validators use a single stream.
- Stores progress in SQLite and syncs bytes before committing recoverable offsets.
- Recovers interrupted downloads as paused. Resume explicitly after reopening.
- `enqueue_paused` implements Download Later: persist a paused, destination-reserving
  entry without any transfer request or temporary file. Call `resume` to start it.
- Pause/cancel drain workers before returning. Cancellation keeps partial data.
- `retry` requeues failed/canceled work and revalidates it before resuming.
- Retries transient failures with exponential backoff and jitter (fallback capped at 120 seconds). `Retry-After` (delta-seconds or HTTP-date) is never shortened. Active downloads keep retrying HTTP 429/503 automatically without exhausting the retry budget. A rate-limited host shares one cooldown window across workers and downloads, and every wait is cancellable.
- `update_url` replaces the source of paused/failed downloads and leaves them paused. Resume reuses checkpoints only when range support, validator, and length still match; otherwise it restarts safely.
- Computes SHA-256 before publication with bounded memory and cancellable reads. `DownloadRequest.expected_sha256` optionally supplies a 64-digit publisher/user checksum. A mismatch fails without publication; retry discards that partial and downloads fresh bytes. Without a reference, `Computed` is distinct from `Verified`.
- `probe` reads the final URL, Content-Disposition filename, content type, length, and range support without enqueueing. Path components are stripped from server filenames; the desktop workflow applies filesystem-specific sanitization.
- `reorder` requires every history/queue ID exactly once.
- `subscribe` provides full snapshots at up to 4 Hz. Refresh using `snapshot` after subscriber lag.
- Zero bandwidth limit means unlimited; concurrency settings are fixed for an engine's lifetime.
- Calls must run inside a Tokio runtime. Call `shutdown` before dropping or reopening the engine.
- An OS file lock enforces one engine owner per database. Shutdown releases ownership.
- Destination parent directories must exist. Existing destination files are never overwritten.
- `enqueue_with_properties` admits optional description/category metadata, deferred
  state, session-only Basic credentials, and same-volume working storage together.
- `update_properties` persists organization metadata without changing transfer identity.
  `update_destination` renames a stopped unpublished job in its current folder;
  changing folders requires no allocated layout/partial. Existing files never move.
- Basic credentials require HTTPS outside loopback, apply only to one download's
  original origin, are stripped across redirects, and are never serialized. Reenter
  them after restart or source replacement using `set_credentials` on paused/failed work.
- Custom temporary storage must support hard links on the destination volume.
  Publication uses a same-directory staging link; no cross-volume copy fallback.
- `remove_download(id, remove_partial_file)` removes fully stopped terminal history. Optional deletion only unlinks the owned temporary name, never a published destination.
- `cleanup_partial_data()` explicitly removes Failed/Canceled partials and entryless partials in the persisted orphan ledger. It never scans arbitrary folders or deletes unrecorded files.
- After removal, `reorder` must contain every remaining ID exactly once; remaining ordering is preserved.

## Storage and resource details

Each worker writes through its own file handle into disjoint offsets in one temporary file. There is no final concatenation. A checkpoint normally occurs every MiB per worker, and on pause/error. SQLite snapshot writes are serialized with state mutation.

Atomic, no-overwrite publication uses a same-directory hard link followed by removal of the temporary name. Use NTFS, ext4, APFS, or another filesystem supporting hard links. FAT/exFAT publication is not supported. Recovery handles a crash between link creation and completion persistence by checking file identity.

The database contains full source URLs, including signed query strings. Do not publish it. Network errors omit request URLs. No telemetry is sent.

## Current limits

- Direct URLs only; explicit session-only Basic auth and origin-scoped captured
  Cookie/Authorization headers are supported. There is no automatic browser-cookie
  export, persistent custom headers, proxy configuration, or media extraction.
- No HTTP response cache: file bodies are streamed to disk once, and durable checkpoints already avoid re-fetching committed ranges.
- SHA-256 verification needs a trustworthy reference supplied by the user/publisher. Computing a hash alone does not establish authenticity; chunk-level corruption cannot be localized without trusted per-chunk hashes.
- Partial cleanup is explicit, not automatic. Unrecorded `.part` files are left alone.
- Process-exit recovery is tested; power-loss directory durability is not guaranteed.
- Other transient errors and standalone metadata probes are bounded by `max_retries`. Persistent rate limiting keeps a download active until recovery, pause, or cancellation.
- Changing the bandwidth limit wakes existing throttle waits; low-speed transfers remain cancellable.
- Durable segment layouts remain fixed while request sizes and active connections adapt. Dynamic work stealing is not implemented; instantaneous speed/ETA may fluctuate.
- Server validators must be truthful. Last-Modified has weaker identity guarantees than strong ETags.
- Schema revision 5 adds durable queue/schedule/capture decisions; older binaries
  reject it rather than bypass unattended-work policy. Revision 4 adds organization metadata and custom working-directory ownership;
  revision 3 added integrity requirements. Revisions 0–3 migrate transactionally with
  safe defaults (revision 4 also migrates to 5); newer revisions are rejected. Older engines reject revision 5 rather
  than losing metadata/storage ownership. Downgrading requires a compatible backup.

## Capture and automation

`enqueue_batch` accepts explicit requests with optional queue assignments, returns
per-item durable IDs/duplicate/error codes, and never infers content identity from
a name. `automation` / `configure_automation` provide named queues, assignments,
start/stop, and persisted UTC schedule slots. Schedule policy and tests are in
[`docs/p2-validation.md`](../../docs/p2-validation.md). Stop/shutdown drain transfer
workers and synchronization probes; all network waits remain cancellable.

Synchronization is opt-in per queue. Completed sources with matching validators
and lengths need no new transfer; changes/unverifiable identity publish a **new
revision filename**, preserving every existing copy. In-place replacement is not
implemented yet. The approved [replacement policy](../../docs/synchronization-replacement.md)
permits an explicit synchronization-only exception for unchanged OSDM-owned files;
ordinary downloads retain conflict detection. Expected checksums are not copied
to a changed version; new revisions
are computed, not falsely marked publisher-verified.

`prepare_capture`, `commit_capture`, and `abort_capture` implement durable,
nonrunnable, expiring browser handoff intent. `set_captured_headers` accepts only
explicit memory-only per-origin headers for a prepared/paused transfer. Ordinary
resume cannot bypass a pending handoff. The desktop owns native browser transport,
startup registration, and opt-in completion actions; transfer protocol/state stays
here. `shutdown_if_idle` atomically refuses runnable work or pending handoffs before
final shutdown so a completion action cannot interrupt a racing enqueue.

## Testing

`cargo test --workspace` runs the fast suite, including checkpoint/publication cancellation, disk-full and permission fault handling, connection caps, schema rejection, and Windows path tests.

The multi-gigabyte streaming and bounded-memory test is ignored by default because it writes about 2.1 GiB and takes several minutes:

```powershell
$env:OSDM_LARGE_TEST = "1"
cargo test -p download-engine --test downloads multi_gigabyte_download_keeps_memory_bounded -- --ignored --nocapture
```

The test skips itself when less than 3 GiB is free. It measures the process working set and fails if it grows by more than 128 MiB during the transfer.

See the root `PLAN.md` for the full product roadmap and pre-release validation checklist.
