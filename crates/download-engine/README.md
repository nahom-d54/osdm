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
- Uses a strong ETag when available, otherwise Last-Modified. Missing validators use a single stream.
- Stores progress in SQLite and syncs bytes before committing recoverable offsets.
- Recovers interrupted downloads as paused. Resume explicitly after reopening.
- Pause/cancel drain workers before returning. Cancellation keeps partial data.
- `retry` requeues failed/canceled work and revalidates it before resuming.
- `reorder` requires every history/queue ID exactly once.
- `subscribe` provides full snapshots at up to 4 Hz. Refresh using `snapshot` after subscriber lag.
- Zero bandwidth limit means unlimited; concurrency settings are fixed for an engine's lifetime.
- Calls must run inside a Tokio runtime. Call `shutdown` before dropping or reopening the engine.
- An OS file lock enforces one engine owner per database. Shutdown releases ownership.
- Destination parent directories must exist. Existing destination files are never overwritten.

## Storage and resource details

Each worker writes through its own file handle into disjoint offsets in one temporary file. There is no final concatenation. A checkpoint normally occurs every MiB per worker, and on pause/error. SQLite snapshot writes are serialized with state mutation.

Atomic, no-overwrite publication uses a same-directory hard link followed by removal of the temporary name. Use NTFS, ext4, APFS, or another filesystem supporting hard links. FAT/exFAT publication is not supported. Recovery handles a crash between link creation and completion persistence by checking file identity.

The database contains full source URLs, including signed query strings. Do not publish it. Network errors omit request URLs. No telemetry is sent.

## Current limits

- Direct URLs only; no browser cookies, custom authentication headers, proxy configuration, or media extraction.
- No expected-checksum input yet; transfer length checks do not prove authenticity.
- No deletion/history-removal command or automatic leftover-part cleanup.
- Process-exit recovery is tested; power-loss directory durability is not guaranteed.
- Numeric Retry-After is honored on transfer retries, capped at 120 seconds; HTTP-date values and probe Retry-After need follow-up.
- Changing the bandwidth limit wakes existing throttle waits; low-speed transfers remain cancellable.
- Fixed segmentation rather than adaptive work stealing; instantaneous speed/ETA may fluctuate.
- Server validators must be truthful. Last-Modified has weaker identity guarantees than strong ETags.
- The initial schema is version 1; compatibility migrations are required before changing persisted models.

See the root `PLAN.md` for the full product roadmap and pre-release validation checklist.
