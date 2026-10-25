import type { KeyboardEvent } from "react";
import { CircleAlert, ExternalLink, FolderOpen, Pause, Play, RotateCcw, Trash, X } from "lucide-react";
import { cn } from "cn";

import { FileBadge } from "@/components/FileBadge";
import { SegmentRibbon } from "@/components/SegmentRibbon";
import { StateChip } from "@/components/StateChip";
import { IconButton } from "@/components/ui/icon-button";
import {
  fileNameOf,
  formatBytes,
  formatEta,
  formatPercent,
  formatSpeed,
  hostOf,
} from "@/lib/format";
import type { Download } from "@/lib/types";
import { downloadedBytes } from "@/lib/types";
import { useEngine } from "@/store/engine";

export function DownloadRow({
  download,
  index,
  selected,
}: {
  download: Download;
  index: number;
  selected: boolean;
}) {
  const { setSelected, toggleDetails, pause, resume, retry, cancel, removeDownload, openFile, revealFile } =
    useEngine();

  const bytes = downloadedBytes(download);
  const live = download.state === "Downloading";
  const percent = formatPercent(bytes, download.total_bytes);
  const sizeLabel =
    download.total_bytes != null
      ? formatBytes(download.total_bytes)
      : bytes > 0
        ? formatBytes(bytes)
        : "—";

  const onKey = (event: KeyboardEvent<HTMLLIElement>) => {
    if (event.target !== event.currentTarget) return;
    if (event.key === "Enter") {
      event.preventDefault();
      toggleDetails();
    }
    if (event.key === " ") {
      event.preventDefault();
      if (download.state === "Downloading" || download.state === "Queued") void pause(download.id);
      else if (download.state === "Paused") void resume(download.id);
    }
  };

  return (
    <li
      data-row={download.id}
      tabIndex={index === 0 ? 0 : -1}
      aria-current={selected ? "true" : undefined}
      onKeyDown={onKey}
      onFocus={(event) => {
        // Clicking a row action focuses that button; only focus on the row
        // itself should select it and open the inspector.
        if (event.target === event.currentTarget) setSelected(download.id);
      }}
      onClick={() => setSelected(download.id)}
      style={{ animationDelay: `${Math.min(index, 9) * 22}ms` }}
      className={cn(
        "row-in group relative cursor-default border-b border-line-soft transition-colors",
        selected ? "bg-selected" : "hover:bg-row-hover",
      )}
    >
      {selected && (
        <span aria-hidden="true" className="absolute inset-y-3 left-0 w-[2px] rounded-full bg-brand" />
      )}

      <div className="flex items-center gap-3 px-4 py-3">
        <FileBadge name={download.destination} />

        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-baseline gap-2">
            <span
              className="truncate text-[13px] font-medium tracking-[-0.005em] text-ink-strong"
              title={download.destination}
            >
              {fileNameOf(download.destination)}
            </span>
            <span aria-hidden="true" className="shrink-0 text-[9px] text-faint/60 @max-[900px]:hidden">
              ·
            </span>
            <span className="truncate font-mono text-[9.5px] text-faint @max-[900px]:hidden" title={download.url}>
              {hostOf(download.url)}
            </span>
          </div>

          <div className="mt-2 flex items-center gap-2.5">
            <div className="min-w-0 flex-1">
              <SegmentRibbon download={download} />
            </div>
            <span className="w-8 shrink-0 text-right font-mono text-[10px] text-muted-foreground tnum">
              {percent}
            </span>
          </div>

          {download.error && (
            <p className="mt-2 flex items-start gap-1.5 text-[10.5px] leading-relaxed text-danger">
              <CircleAlert className="mt-px size-3 shrink-0" strokeWidth={1.75} aria-hidden="true" />
              <span>
                {sentence(download.error)}{" "}
                <span className="text-danger/70">
                  {bytes > 0
                    ? `${formatBytes(bytes)} is committed — retry to continue.`
                    : "Retry when the server is reachable."}
                </span>
              </span>
            </p>
          )}
        </div>

        <div className="w-[92px] shrink-0">
          <StateChip state={download.state} />
        </div>

        <div className="hidden w-[76px] shrink-0 text-right font-mono text-[10.5px] text-muted-foreground tnum @min-[760px]:block">
          {live ? formatSpeed(download.bytes_per_second) : ""}
        </div>

        <div className="hidden w-[62px] shrink-0 text-right font-mono text-[10.5px] text-faint tnum @min-[920px]:block">
          {live ? formatEta(download.eta_seconds) : ""}
        </div>

        <div className="w-[70px] shrink-0 text-right font-mono text-[10.5px] text-muted-foreground tnum">
          {sizeLabel}
        </div>

        <div
          onClick={(event) => event.stopPropagation()}
          className={cn(
            "flex w-[56px] shrink-0 items-center justify-end gap-0.5 transition-opacity",
            selected
              ? "opacity-100"
              : "pointer-events-none opacity-0 group-hover:pointer-events-auto group-hover:opacity-100 group-focus-within:pointer-events-auto group-focus-within:opacity-100 [@media(hover:none)]:pointer-events-auto [@media(hover:none)]:opacity-100",
          )}
        >
          {(download.state === "Downloading" || download.state === "Queued") && (
            <IconButton
              label={download.state === "Queued" ? "Pause (hold in queue)" : "Pause"}
              onClick={() => void pause(download.id)}
              size="icon-xs"
            >
              <Pause strokeWidth={1.75} />
            </IconButton>
          )}
          {download.state === "Paused" && (
            <IconButton label="Resume" onClick={() => void resume(download.id)} size="icon-xs">
              <Play strokeWidth={1.75} />
            </IconButton>
          )}
          {["Downloading", "Queued", "Paused"].includes(download.state) && (
            <IconButton
              label={download.state === "Downloading" ? "Stop" : "Cancel download"}
              onClick={() => void cancel(download.id)}
              size="icon-xs"
              className="hover:text-danger"
            >
              <X strokeWidth={1.75} />
            </IconButton>
          )}
          {(download.state === "Failed" || download.state === "Canceled") && (
            <IconButton label="Retry" onClick={() => void retry(download.id)} size="icon-xs">
              <RotateCcw strokeWidth={1.75} />
            </IconButton>
          )}
          {download.state === "Completed" && (
            <>
              <IconButton label="Open file" onClick={() => void openFile(download.id)} size="icon-xs">
                <ExternalLink strokeWidth={1.75} />
              </IconButton>
              <IconButton label="Show in folder" onClick={() => void revealFile(download.id)} size="icon-xs">
                <FolderOpen strokeWidth={1.75} />
              </IconButton>
            </>
          )}
          {(download.state === "Failed" || download.state === "Canceled") && (
            <IconButton
              label="Remove entry, keep partial file"
              onClick={() => void removeDownload(download.id, false)}
              size="icon-xs"
              className="hover:text-danger"
            >
              <Trash strokeWidth={1.75} />
            </IconButton>
          )}
        </div>
      </div>
    </li>
  );
}

function sentence(value: string): string {
  if (!value) return value;
  const trimmed = value.trim();
  const capitalized = trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
  return /[.!?]$/.test(capitalized) ? capitalized : `${capitalized}.`;
}
