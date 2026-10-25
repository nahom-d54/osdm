import type { DownloadState } from "@/lib/types";
import { cn } from "cn";

const META: Record<DownloadState, { label: string; tone: string; dot: string; live?: boolean }> = {
  Queued: {
    label: "Queued",
    tone: "text-muted-foreground",
    dot: "border border-faint bg-transparent",
  },
  Downloading: {
    label: "Downloading",
    tone: "text-brand",
    dot: "bg-brand",
    live: true,
  },
  Pausing: {
    label: "Pausing",
    tone: "text-warn",
    dot: "bg-warn",
  },
  Paused: {
    label: "Paused",
    tone: "text-warn",
    dot: "bg-warn",
  },
  Canceling: {
    label: "Stopping",
    tone: "text-danger",
    dot: "bg-danger",
  },
  Canceled: {
    label: "Stopped",
    tone: "text-muted-foreground",
    dot: "bg-faint",
  },
  Failed: {
    label: "Failed",
    tone: "text-danger",
    dot: "bg-danger",
  },
  Completed: {
    label: "Completed",
    tone: "text-ok",
    dot: "bg-ok",
  },
};

/** State is always written out; colour is a second, never the only, cue. */
export function StateChip({ state }: { state: DownloadState }) {
  const meta = META[state];
  return (
    <span className={cn("inline-flex items-center gap-1.5 whitespace-nowrap text-[11px] font-medium", meta.tone)}>
      <span
        aria-hidden="true"
        className={cn("size-1.5 shrink-0 rounded-full", meta.dot, meta.live && "breathe")}
      />
      {meta.label}
    </span>
  );
}
