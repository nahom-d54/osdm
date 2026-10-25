import type { Download, DownloadState } from "@/lib/types";
import { committedBytes, downloadedBytes } from "@/lib/types";
import { formatBytes, formatPercent } from "@/lib/format";
import { cn } from "cn";

type Tone = "active" | "done" | "failed" | "idle";

const TONE: Record<Tone, { fill: string; live: string; label: string }> = {
  active: { fill: "bg-brand", live: "bg-brand/55", label: "In flight" },
  done: { fill: "bg-ok", live: "bg-ok/70", label: "Committed" },
  failed: { fill: "bg-danger/80", live: "bg-danger/45", label: "In flight" },
  idle: { fill: "bg-faint", live: "bg-faint/50", label: "In flight" },
};

function toneFor(state: DownloadState): Tone {
  if (state === "Completed") return "done";
  if (state === "Failed") return "failed";
  if (state === "Paused" || state === "Pausing" || state === "Canceled" || state === "Canceling") return "idle";
  return "active";
}

function clampPercent(value: number): number {
  return Math.max(0, Math.min(100, value));
}

function isKnown(download: Download): boolean {
  return (
    download.total_bytes != null &&
    download.total_bytes > 0 &&
    download.segments.length > 0 &&
    download.segments.every((segment) => segment.end != null)
  );
}

/**
 * The lane field: one block per engine segment, never an invented bar.
 *
 *   committed → solid signal   (durable; survives a restart)
 *   live      → moving signal  (in flight; reset on recovery)
 *   remaining → quiet track
 */
export function SegmentRibbon({ download }: { download: Download }) {
  const tone = TONE[toneFor(download.state)];
  const known = isKnown(download);
  const total = download.total_bytes ?? 0;
  const committed = committedBytes(download);
  const downloaded = downloadedBytes(download);
  const percent = formatPercent(committed, download.total_bytes);
  const flowing = download.state === "Downloading";

  if (!known) {
    const probing = download.state === "Downloading" || download.state === "Queued" || download.state === "Pausing";
    return (
      <div
        role="progressbar"
        aria-label="Download progress"
        aria-valuemin={0}
        aria-valuetext={`${formatBytes(downloaded)} downloaded`}
        className={cn(
          "h-[6px] w-full overflow-hidden rounded-[2px] bg-line-soft",
          probing && flowing && "lane-indeterminate",
        )}
      >
        {!flowing && downloaded > 0 ? (
          <div className={cn("h-full rounded-[2px]", tone.fill)} style={{ width: "12%" }} />
        ) : null}
      </div>
    );
  }

  return (
    <div
      role="progressbar"
      aria-label="Download progress, committed bytes"
      aria-valuemin={0}
      aria-valuemax={total}
      aria-valuenow={Math.round(committed)}
      aria-valuetext={`${percent || "0%"} committed, ${formatBytes(committed)} of ${formatBytes(total)}`}
      className="flex h-[6px] w-full gap-[2px] overflow-hidden rounded-[2px]"
    >
      {download.segments.map((segment, index) => {
        const size = (segment.end ?? 0) - segment.start;
        if (size <= 0) return null;
        const committedWidth = clampPercent((segment.committed / size) * 100);
        const liveWidth = clampPercent(((segment.downloaded - segment.committed) / size) * 100);
        return (
          <div
            key={index}
            className="relative h-full overflow-hidden rounded-[2px] bg-line-soft"
            style={{ flexGrow: size, flexBasis: 0 }}
          >
            <div
              className={cn("absolute inset-y-0 left-0 transition-[width] duration-200 ease-linear", tone.fill)}
              style={{ width: `${committedWidth}%` }}
            />
            {liveWidth > 0 && (
              <div
                className={cn(
                  "absolute inset-y-0 transition-[width,left] duration-200 ease-linear",
                  tone.live,
                  flowing && "lane-live",
                )}
                style={{ left: `${committedWidth}%`, width: `${liveWidth}%` }}
              />
            )}
            {flowing && committedWidth + liveWidth > 0 && (
              <span
                className="lane-head top-0 bottom-0"
                style={{ left: `${Math.min(100, committedWidth + liveWidth)}%` }}
                aria-hidden="true"
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

/**
 * The same field at inspection scale: one lane per connection, with the
 * durable/live boundary readable lane by lane.
 */
export function SegmentLanes({ download }: { download: Download }) {
  const tone = TONE[toneFor(download.state)];
  const known = isKnown(download);
  const flowing = download.state === "Downloading";

  if (!known) {
    return (
      <div className="flex flex-col gap-2.5">
        <div className="flex items-center gap-3">
          <span className="w-4 shrink-0 font-mono text-[9px] text-faint tnum">01</span>
          <div
            className={cn(
              "relative h-2 flex-1 overflow-hidden rounded-[3px] bg-line-soft",
              flowing && "lane-indeterminate",
            )}
          >
            {!flowing && (
              <div className={cn("absolute inset-y-0 left-0 rounded-[3px]", tone.fill)} style={{ width: "10%" }} />
            )}
          </div>
          <span className="w-[92px] shrink-0 text-right font-mono text-[9.5px] text-muted-foreground tnum">
            {formatBytes(downloadedBytes(download))}
          </span>
        </div>
        <p className="pl-7 text-[10.5px] leading-relaxed text-muted-foreground">
          Single stream, length not yet known. The engine probes before it splits the transfer.
        </p>
      </div>
    );
  }

  const total = download.total_bytes!;

  return (
    <div className="flex flex-col gap-2.5">
      {download.segments.map((segment, index) => {
        const size = (segment.end ?? 0) - segment.start;
        const committedWidth = clampPercent((segment.committed / Math.max(1, size)) * 100);
        const liveWidth = clampPercent(((segment.downloaded - segment.committed) / Math.max(1, size)) * 100);
        return (
          <div
            key={index}
            className="flex items-center gap-3"
            title={`Bytes ${formatBytes(segment.start)} – ${formatBytes(segment.end ?? 0)}`}
          >
            <span className="w-4 shrink-0 font-mono text-[9px] text-faint tnum">
              {String(index + 1).padStart(2, "0")}
            </span>
            <div className="relative h-2 flex-1 overflow-hidden rounded-[3px] bg-line-soft">
              <div
                className={cn("absolute inset-y-0 left-0 transition-[width] duration-200 ease-linear", tone.fill)}
                style={{ width: `${committedWidth}%` }}
              />
              {liveWidth > 0 && (
                <div
                  className={cn(
                    "absolute inset-y-0 transition-[width,left] duration-200 ease-linear",
                    tone.live,
                    flowing && "lane-live",
                  )}
                  style={{ left: `${committedWidth}%`, width: `${liveWidth}%` }}
                />
              )}
              {flowing && committedWidth + liveWidth > 0 && (
                <span
                  className="lane-head top-0 bottom-0"
                  style={{ left: `${Math.min(100, committedWidth + liveWidth)}%` }}
                  aria-hidden="true"
                />
              )}
            </div>
            <span className="w-[92px] shrink-0 text-right font-mono text-[9.5px] text-muted-foreground tnum">
              {formatBytes(segment.committed)} / {formatBytes(size)}
            </span>
          </div>
        );
      })}

      <div className="mt-1 flex flex-wrap items-center gap-x-4 gap-y-1.5 pl-7 font-mono text-[8.5px] uppercase tracking-[0.08em] text-faint">
        <Legend className={tone.fill} label="Committed" />
        <Legend className={tone.live} label="In flight" />
        <Legend className="bg-line-soft" label="Remaining" />
        <span className="ml-auto normal-case tracking-normal">
          {download.segments.length} {download.segments.length === 1 ? "lane" : "lanes"} ·{" "}
          {formatBytes(total)} total
        </span>
      </div>
    </div>
  );
}

function Legend({ className, label }: { className: string; label: string }) {
  return (
    <span className="inline-flex items-center gap-1.5 normal-case tracking-normal">
      <span aria-hidden="true" className={cn("size-1.5 rounded-[1px]", className)} />
      {label}
    </span>
  );
}
