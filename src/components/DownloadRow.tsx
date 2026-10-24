import type { Download } from "@/lib/types";
import { downloadedBytes } from "@/lib/types";
import { formatBytes, formatEta, formatPercent, formatSpeed } from "@/lib/format";
import { StateChip } from "@/components/StateChip";
import { SegmentRibbon } from "@/components/SegmentRibbon";
import {
  ChevronDownIcon,
  ChevronUpIcon,
  FolderIcon,
  OpenIcon,
  PauseIcon,
  PlayIcon,
  RetryIcon,
  TrashIcon,
  XIcon,
} from "@/components/icons";
import { IconButton } from "@/components/ui/Button";
import { useEngine } from "@/store/engine";
import type { KeyboardEvent } from "react";

const QUEUED_STATES = new Set(["Queued", "Paused"]);
const DELETABLE_STATES = new Set(["Completed", "Failed", "Canceled"]);

export function DownloadRow({ download, index, selected }: { download: Download; index: number; selected: boolean }) {
  const { setSelected, toggleDetails, pause, resume, retry, cancel, move, removeDownload, openFile, revealFile } =
    useEngine();
  const bytes = downloadedBytes(download);
  const live = download.state === "Downloading";
  const inQueue = QUEUED_STATES.has(download.state);

  const onKey = (event: KeyboardEvent<HTMLLIElement>) => {
    if (event.target !== event.currentTarget) return; // child buttons handle their own keys
    if (event.key === "Enter") {
      event.preventDefault();
      toggleDetails();
    }
    if (event.key === " ") {
      // Space toggles pause/resume when the row offers one.
      const button = event.currentTarget.querySelector<HTMLButtonElement>(
        'button[title="Pause"], button[title="Resume"]',
      );
      button?.click();
    }
  };

  return (
    <li
      data-row={download.id}
      tabIndex={index === 0 ? 0 : -1}
      onKeyDown={onKey}
      onFocus={() => setSelected(download.id)}
      className={`group flex cursor-default items-center gap-3 rounded-[var(--radius-panel)] px-3 py-2.5 transition-colors ${
        selected ? "bg-raise ring-1 ring-line" : "hover:bg-panel"
      }`}
    >
      <span className="w-5 shrink-0 text-right font-mono text-[10px] text-faint" aria-label="Queue position">
        {inQueue ? index + 1 : ""}
      </span>

      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span className="shrink-0 max-w-72 truncate text-[13px] font-medium text-ink-strong" title={download.destination}>
            {fileName(download.destination)}
          </span>
          <span className="truncate font-mono text-[10.5px] text-faint" title={download.url}>
            {host(download.url)}
          </span>
        </div>
        <div className="mt-1.5 flex items-center gap-2">
          <SegmentRibbon download={download} />
          <span className="w-14 shrink-0 text-right font-mono text-[11px] text-muted">{formatBytes(bytes)}</span>
          <span className="w-11 shrink-0 text-right font-mono text-[11px] text-faint">
            {formatPercent(bytes, download.total_bytes)}
          </span>
        </div>
        {download.error && (
          <p className="mt-1.5 text-[11.5px] text-danger">
            {download.error} — safe progress is kept. Retry to continue.
          </p>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-3">
        <StateChip state={download.state} />
        <span className="w-16 text-right font-mono text-[11px] text-muted" aria-label="Transfer rate">
          {live ? formatSpeed(download.bytes_per_second) : ""}
        </span>
        <span className="w-14 text-right font-mono text-[11px] text-faint" aria-label="Estimated time remaining">
          {live ? formatEta(download.eta_seconds) : ""}
        </span>

        <div className="flex items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
          {inQueue && (
            <>
              <IconButton label="Move up in queue" onClick={() => void move(download.id, "up")}>
                <ChevronUpIcon size={12} />
              </IconButton>
              <IconButton label="Move down in queue" onClick={() => void move(download.id, "down")}>
                <ChevronDownIcon size={12} />
              </IconButton>
            </>
          )}
          {(download.state === "Downloading" || download.state === "Queued") && (
            <IconButton label="Pause" onClick={() => void pause(download.id)}>
              <PauseIcon size={12} />
            </IconButton>
          )}
          {download.state === "Paused" && (
            <IconButton label="Resume" onClick={() => void resume(download.id)}>
              <PlayIcon size={12} />
            </IconButton>
          )}
          {live && (
            <IconButton label="Stop" onClick={() => void cancel(download.id)}>
              <XIcon size={12} />
            </IconButton>
          )}
          {(download.state === "Failed" || download.state === "Canceled") && (
            <IconButton label="Retry" onClick={() => void retry(download.id)}>
              <RetryIcon size={12} />
            </IconButton>
          )}
          {download.state === "Completed" && (
            <>
              <IconButton label="Open" onClick={() => void openFile(download.id)}>
                <OpenIcon size={12} />
              </IconButton>
              <IconButton label="Show in folder" onClick={() => void revealFile(download.id)}>
                <FolderIcon size={12} />
              </IconButton>
            </>
          )}
          {DELETABLE_STATES.has(download.state) && (
            <IconButton
              label="Delete entry"
              onClick={() => void removeDownload(download.id, false)}
            >
              <TrashIcon size={12} />
            </IconButton>
          )}
        </div>
      </div>
    </li>
  );
}

function fileName(path: string): string {
  const cleaned = path.replace(/\\/g, "/");
  return cleaned.slice(cleaned.lastIndexOf("/") + 1) || path;
}

function host(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
