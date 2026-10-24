import type { Download } from "@/lib/types";
import { committedBytes, downloadedBytes } from "@/lib/types";

/**
 * OSDM's signature element. One ribbon per download, built from the
 * engine's actual segment list:
 *
 *   committed bytes → solid fill      (durable; survives a crash)
 *   live bytes → translucent pulse    (in flight; reset on recovery)
 *   remaining → raise with a faint keyline
 *
 * The ribbon never represents complete when any byte is merely live.
 */
export function SegmentRibbon({ download }: { download: Download }) {
  const segments = download.segments;
  const total = download.total_bytes;
  const drained = download.state === "Paused" || download.state === "Canceling" || download.state === "Canceled";
  const knownSize = total != null && total > 0;

  // Single streaming transfer of unknown length: indeterminate striping while live.
  if (!knownSize || segments.length === 0) {
    const flowing = download.state === "Downloading" && !drained;
    return (
      <div
        className="ribbon flex h-1.5 overflow-hidden rounded-full bg-raise"
        role="progressbar"
        aria-label="Download progress"
      >
        {flowing ? (
          <div className="flow-stripes w-full bg-accent-soft" />
        ) : (
          <div className="bg-line" style={{ width: `${Math.min(90, (downloadedBytes(download) / Math.max(1, total ?? downloadedBytes(download))) * 100)}%` }} />
        )}
      </div>
    );
  }

  // Cover the tiny window before the probe assigns segments/length.
  if (segments.some((s) => s.end == null)) {
    return (
      <div className="ribbon flex h-1.5 overflow-hidden rounded-full bg-raise" role="progressbar" aria-label="Probing resource">
        <div className="flow-stripes w-full bg-accent-soft" />
      </div>
    );
  }

  return (
    <div
      className="ribbon flex h-1.5 overflow-hidden rounded-full bg-raise"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={total}
      aria-valuenow={committedBytes(download)}
      aria-label="Download progress, committed bytes"
    >
      {segments.map((seg, index) => {
        const size = seg.end! - seg.start;
        if (size <= 0) return null;
        const committedWidth = (seg.committed / size) * 100;
        const liveWidth = ((seg.downloaded - seg.committed) / size) * 100;
        const flowing = liveWidth > 0 && download.state === "Downloading" && !drained;
        return (
          <div
            key={index}
            className="relative h-full shrink-0 grow-0"
            style={{ width: `${(size / total) * 100}%` }}
          >
            {/* committed */}
            <div
              className={`absolute inset-y-0 left-0 bg-ok ${download.state === "Failed" ? "opacity-80" : ""}`}
              style={{ width: `${committedWidth}%` }}
            />
            {/* live, above committed only where not yet committed */}
            {flowing && (
              <div
                className="ribbon-live absolute inset-y-0 bg-accent"
                style={{ left: `${committedWidth}%`, width: `${liveWidth}%` }}
              />
            )}
            {index > 0 && seg.start > 0 && (
              <div aria-hidden="true" className="absolute inset-y-0 left-0 w-px bg-bg/70" />
            )}
          </div>
        );
      })}
    </div>
  );
}
