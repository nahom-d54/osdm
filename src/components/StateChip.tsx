import type { DownloadState } from "@/lib/types";

const LABELS: Record<DownloadState, string> = {
  Queued: "Queued",
  Downloading: "Downloading",
  Pausing: "Pausing",
  Paused: "Paused",
  Canceling: "Stopping",
  Canceled: "Stopped",
  Failed: "Failed",
  Completed: "Completed",
};

const COLORS: Record<DownloadState, { dot: string; chip: string }> = {
  Queued: { dot: "bg-faint", chip: "text-muted bg-raise" },
  Downloading: { dot: "bg-accent", chip: "text-accent bg-accent-soft" },
  Pausing: { dot: "bg-warn", chip: "text-warn bg-warn-soft" },
  Paused: { dot: "bg-warn", chip: "text-warn bg-warn-soft" },
  Canceling: { dot: "bg-danger", chip: "text-danger bg-danger-soft" },
  Canceled: { dot: "bg-faint", chip: "text-muted bg-raise" },
  Failed: { dot: "bg-danger", chip: "text-danger bg-danger-soft" },
  Completed: { dot: "bg-ok", chip: "text-ok bg-ok-soft" },
};

/** State chip with a text label — never color alone (PLAN 5.4). */
export function StateChip({ state }: { state: DownloadState }) {
  const colors = COLORS[state];
  const live = state === "Downloading";
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium ${colors.chip}`}
    >
      <span
        className={`h-1.5 w-1.5 rounded-full ${colors.dot} ${live ? "animate-pulse" : ""}`}
        aria-hidden="true"
      />
      {LABELS[state]}
    </span>
  );
}
