import { useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronUp,
  CircleAlert,
  Copy,
  ExternalLink,
  FolderOpen,
  PanelRight,
  Pause,
  Play,
  RotateCcw,
  Trash,
  X,
} from "lucide-react";
import { cn } from "cn";

import { FileBadge } from "@/components/FileBadge";
import { SegmentLanes } from "@/components/SegmentRibbon";
import { StateChip } from "@/components/StateChip";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Modal } from "@/components/ui/modal";
import {
  fileNameOf,
  formatBytes,
  formatEta,
  formatSpeed,
  hostOf,
} from "@/lib/format";
import type { Download } from "@/lib/types";
import { committedBytes, downloadedBytes } from "@/lib/types";
import { useEngine } from "@/store/engine";

export function DetailsPanel({ download }: { download: Download | null }) {
  const { pause, resume, retry, cancel, openFile, revealFile, removeDownload, move, toggleDetails, downloads } =
    useEngine();
  const [confirmOpen, setConfirmOpen] = useState(false);

  if (!download) {
    return (
      <aside className="flex h-full w-[360px] shrink-0 flex-col border-l border-line-soft bg-panel max-[1319px]:absolute max-[1319px]:inset-y-0 max-[1319px]:right-0 max-[1319px]:z-40 max-[1319px]:shadow-[-24px_0_60px_-30px_rgba(0,0,0,0.7)] max-[520px]:w-full">
        <PanelHeader onClose={toggleDetails} />
        <div className="flex flex-1 flex-col items-center justify-center gap-3 px-8 text-center">
          <PanelRight className="size-6 text-faint" strokeWidth={1.5} aria-hidden="true" />
          <p className="font-display text-[15px] font-semibold text-ink-strong">Nothing selected</p>
          <p className="max-w-[15rem] text-[11.5px] leading-relaxed text-muted-foreground">
            Choose a transfer to inspect its connection lanes, checkpoints, and file location.
          </p>
        </div>
      </aside>
    );
  }

  const committed = committedBytes(download);
  const received = downloadedBytes(download);
  const known = download.total_bytes != null && download.total_bytes > 0;
  const percent = known ? Math.min(100, Math.floor((committed / download.total_bytes!) * 100)) : null;
  const live = download.state === "Downloading";
  const queue = downloads.filter((item) => item.state === "Queued" || item.state === "Paused");
  const queueIndex = queue.findIndex((item) => item.id === download.id);
  const removable = ["Completed", "Failed", "Canceled"].includes(download.state);
  const completed = download.state === "Completed";

  const primary = (() => {
    if (download.state === "Downloading" || download.state === "Queued") {
      return (
        <Button onClick={() => void pause(download.id)} className="flex-1 justify-center" size="sm">
          <Pause strokeWidth={1.75} />
          Pause
        </Button>
      );
    }
    if (download.state === "Paused") {
      return (
        <Button onClick={() => void resume(download.id)} className="flex-1 justify-center" size="sm">
          <Play strokeWidth={1.75} />
          Resume
        </Button>
      );
    }
    if (download.state === "Failed" || download.state === "Canceled") {
      return (
        <Button onClick={() => void retry(download.id)} className="flex-1 justify-center" size="sm">
          <RotateCcw strokeWidth={1.75} />
          Retry
        </Button>
      );
    }
    if (completed) {
      return (
        <Button onClick={() => void openFile(download.id)} className="flex-1 justify-center" size="sm">
          <ExternalLink strokeWidth={1.75} />
          Open file
        </Button>
      );
    }
    return (
      <Button className="flex-1 justify-center" size="sm" disabled>
        {download.state === "Pausing" ? "Pausing…" : "Stopping…"}
      </Button>
    );
  })();

  const secondary = (() => {
    if (["Downloading", "Queued", "Paused"].includes(download.state)) {
      return (
        <Button
          variant="secondary"
          onClick={() => void cancel(download.id)}
          className="flex-1 justify-center hover:border-danger/40 hover:text-danger"
          size="sm"
        >
          <X strokeWidth={1.75} />
          Stop
        </Button>
      );
    }
    if (completed) {
      return (
        <Button
          variant="secondary"
          onClick={() => void revealFile(download.id)}
          className="flex-1 justify-center"
          size="sm"
        >
          <FolderOpen strokeWidth={1.75} />
          Folder
        </Button>
      );
    }
    return null;
  })();

  const remove = async () => {
    await removeDownload(download.id, true);
    setConfirmOpen(false);
  };

  return (
    <aside
      aria-label="Download details"
      className="panel-in flex h-full w-[360px] shrink-0 flex-col border-l border-line-soft bg-panel max-[1319px]:absolute max-[1319px]:inset-y-0 max-[1319px]:right-0 max-[1319px]:z-40 max-[1319px]:shadow-[-24px_0_60px_-30px_rgba(0,0,0,0.7)] max-[520px]:w-full"
    >
      <PanelHeader onClose={toggleDetails} />

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="border-b border-line-soft px-4 pt-4 pb-4">
          <div className="flex items-start gap-3">
            <FileBadge name={download.destination} size="lg" />
            <div className="min-w-0 flex-1">
              <h2
                className="break-words font-display text-[16px] font-semibold leading-tight tracking-[-0.02em] text-ink-strong"
                title={download.destination}
              >
                {fileNameOf(download.destination)}
              </h2>
              <p className="mt-1.5 truncate font-mono text-[9.5px] text-faint" title={download.url}>
                {hostOf(download.url)}
              </p>
            </div>
          </div>

          <div className="mt-4 flex gap-2">
            {primary}
            {secondary}
          </div>
        </div>

        {download.error && (
          <div className="mx-4 mt-4 rounded-md border border-danger/25 bg-danger-soft px-3 py-2.5">
            <div className="flex items-start gap-2">
              <CircleAlert className="mt-px size-3.5 shrink-0 text-danger" strokeWidth={1.75} aria-hidden="true" />
              <div>
                <p className="text-[11.5px] font-medium leading-relaxed text-danger">{sentence(download.error)}</p>
                <p className="mt-1 text-[10.5px] leading-relaxed text-danger/75">
                  {committed > 0
                    ? `${formatBytes(committed)} is safe on disk. Retrying continues from that checkpoint.`
                    : "No progress was committed. Retry when the server is reachable."}
                </p>
              </div>
            </div>
          </div>
        )}

        <section className="border-b border-line-soft px-4 py-4">
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="flex items-baseline gap-1">
                <span
                  className={cn(
                    "font-display font-semibold leading-none tracking-[-0.05em] text-ink-strong tnum",
                    known ? "text-[44px]" : "text-[26px]",
                  )}
                >
                  {known ? percent : formatBytes(received)}
                </span>
                {known ? <span className="font-display text-[16px] font-semibold text-faint">%</span> : null}
              </div>
              <p className="mt-2 font-mono text-[9.5px] leading-relaxed text-muted-foreground tnum">
                {known
                  ? `${formatBytes(committed)} committed of ${formatBytes(download.total_bytes!)}`
                  : `Length unknown · ${formatBytes(received)} received`}
              </p>
            </div>
            <StateChip state={download.state} />
          </div>

          <dl className="mt-4 grid grid-cols-3 divide-x divide-line-soft border-t border-line-soft pt-3">
            <Stat label="Rate" value={live ? formatSpeed(download.bytes_per_second) : "—"} />
            <Stat
              label="Remaining"
              value={
                completed
                  ? "Done"
                  : live
                    ? formatEta(download.eta_seconds)
                    : known
                      ? formatBytes(Math.max(0, download.total_bytes! - committed))
                      : "—"
              }
              className="pl-3"
            />
            <Stat
              label="Size"
              value={known ? formatBytes(download.total_bytes!) : received > 0 ? formatBytes(received) : "Unknown"}
              className="pl-3"
            />
          </dl>
        </section>

        <section className="border-b border-line-soft px-4 py-4">
          <div className="mb-3 flex items-baseline justify-between gap-2">
            <h3 className="eyebrow">Connection lanes</h3>
            <span className="font-mono text-[9px] text-faint">
              {download.ranged ? "range requests" : "single stream"}
            </span>
          </div>
          <SegmentLanes download={download} />
        </section>

        <section className="border-b border-line-soft px-4 py-4">
          <h3 className="eyebrow mb-2">Manifest</h3>
          <dl className="divide-y divide-line-soft">
            <ManifestRow label="Origin" value={download.url} copyable />
            <ManifestRow label="Destination" value={download.destination} copyable />
            <ManifestRow label="Temp file" value={download.temporary_path} copyable />
            <ManifestRow
              label="Validator"
              value={download.validator ?? "None supplied by the server"}
              muted={!download.validator}
            />
          </dl>
        </section>

        {queueIndex >= 0 && (
          <section className="border-b border-line-soft px-4 py-4">
            <div className="flex items-center justify-between gap-2">
              <div>
                <h3 className="eyebrow">Queue</h3>
                <p className="mt-1.5 text-[11px] text-muted-foreground">
                  Position {queueIndex + 1} of {queue.length}
                </p>
              </div>
              <div className="flex items-center gap-0.5">
                <IconButton
                  label="Move up in queue"
                  disabled={queueIndex <= 0}
                  onClick={() => void move(download.id, "up")}
                >
                  <ChevronUp strokeWidth={1.75} />
                </IconButton>
                <IconButton
                  label="Move down in queue"
                  disabled={queueIndex >= queue.length - 1}
                  onClick={() => void move(download.id, "down")}
                >
                  <ChevronDown strokeWidth={1.75} />
                </IconButton>
              </div>
            </div>
          </section>
        )}

        {removable && (
          <section className="px-4 py-4">
            <Button
              variant="ghost"
              onClick={() => setConfirmOpen(true)}
              className="w-full justify-start text-danger hover:bg-danger-soft hover:text-danger"
            >
              <Trash strokeWidth={1.75} />
              {completed ? "Remove from history" : "Delete download"}
            </Button>
            <p className="mt-1.5 px-2 text-[10px] leading-relaxed text-faint">
              {completed
                ? "Removes the entry from OSDM. The downloaded file stays on disk."
                : "Deletes the entry and its working file. The destination is never touched."}
            </p>
          </section>
        )}
      </div>

      <Modal
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={completed ? "Remove from history?" : "Delete download?"}
        description={
          completed
            ? "The downloaded file is not touched. You can add the link again at any time."
            : "The partial data for this transfer is deleted. This cannot be undone."
        }
        className="sm:max-w-[26rem]"
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirmOpen(false)}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={() => void remove()}>
              <Trash strokeWidth={1.75} />
              {completed ? "Remove entry" : "Delete download"}
            </Button>
          </>
        }
      >
        <p className="break-all font-mono text-[11px] leading-relaxed text-muted-foreground">
          {completed ? download.destination : download.temporary_path}
        </p>
      </Modal>
    </aside>
  );
}

function PanelHeader({ onClose }: { onClose: () => void }) {
  return (
    <div className="flex h-12 shrink-0 items-center justify-between border-b border-line-soft px-4">
      <span className="eyebrow">Inspector</span>
      <IconButton label="Close inspector" onClick={onClose}>
        <X strokeWidth={1.75} />
      </IconButton>
    </div>
  );
}

function Stat({ label, value, className }: { label: string; value: string; className?: string }) {
  return (
    <div className={cn("min-w-0", className)}>
      <dt className="eyebrow">{label}</dt>
      <dd className="mt-1 truncate font-mono text-[11px] text-foreground tnum" title={value}>
        {value}
      </dd>
    </div>
  );
}

function ManifestRow({
  label,
  value,
  copyable,
  muted,
}: {
  label: string;
  value: string;
  copyable?: boolean;
  muted?: boolean;
}) {
  return (
    <div className="grid grid-cols-[62px_minmax(0,1fr)_auto] items-start gap-2 py-2">
      <dt className="pt-0.5 font-mono text-[8.5px] uppercase tracking-[0.1em] text-faint">{label}</dt>
      <dd
        className={cn(
          "min-w-0 truncate font-mono text-[9.5px]",
          muted ? "text-faint" : "text-muted-foreground",
        )}
        title={value}
      >
        {value}
      </dd>
      {copyable ? <CopyButton value={value} label={label} /> : <span className="size-6" />}
    </div>
  );
}

function CopyButton({ value, label }: { value: string; label: string }) {
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    const ok = await writeClipboard(value);
    setCopied(ok);
    if (ok) window.setTimeout(() => setCopied(false), 1400);
  };

  return (
    <IconButton label={copied ? "Copied" : `Copy ${label.toLowerCase()}`} onClick={() => void copy()} size="icon-xs">
      {copied ? <Check className="text-ok" strokeWidth={2} /> : <Copy strokeWidth={1.75} />}
    </IconButton>
  );
}

async function writeClipboard(value: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(value);
    return true;
  } catch {
    // Webviews without a clipboard permission still have the legacy path.
    try {
      const area = document.createElement("textarea");
      area.value = value;
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.appendChild(area);
      area.select();
      const ok = document.execCommand("copy");
      area.remove();
      return ok;
    } catch {
      return false;
    }
  }
}

function sentence(value: string): string {
  if (!value) return value;
  const trimmed = value.trim();
  const capitalized = trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
  return /[.!?]$/.test(capitalized) ? capitalized : `${capitalized}.`;
}
