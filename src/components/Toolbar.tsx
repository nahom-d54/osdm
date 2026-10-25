import type { RefObject } from "react";
import { PanelRight, Pause, Play, Plus, Search, X } from "lucide-react";
import { cn } from "cn";

import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { Kbd } from "@/components/ui/kbd";
import { formatBytes, formatSpeed, plural } from "@/lib/format";
import { VIEW_COPY, groupFor, matchesQuery, matchesView } from "@/lib/views";
import { useEngine } from "@/store/engine";

interface ToolbarProps {
  searchRef: RefObject<HTMLInputElement | null>;
  onAdd: () => void;
}

export function Toolbar({ searchRef, onAdd }: ToolbarProps) {
  const {
    downloads,
    view,
    query,
    setQuery,
    pauseAll,
    resumeAll,
    stopAll,
    detailsOpen,
    toggleDetails,
  } = useEngine();

  const moving = downloads.filter((download) => download.state === "Downloading" || download.state === "Pausing").length;
  const waiting = downloads.filter((download) => download.state === "Queued" || download.state === "Paused").length;
  const paused = downloads.filter((download) => download.state === "Paused").length;
  const stoppable = downloads.filter((download) =>
    ["Queued", "Downloading", "Pausing", "Paused"].includes(download.state),
  ).length;
  const pausable = downloads.filter(
    (download) => download.state === "Downloading" || download.state === "Queued",
  ).length;
  const completed = downloads.filter((download) => download.state === "Completed");
  const failed = downloads.filter((download) => download.state === "Failed").length;
  const terminal = downloads.filter((download) =>
    ["Completed", "Failed", "Canceled"].includes(download.state),
  ).length;
  const throughput = downloads.reduce(
    (sum, download) => (download.state === "Downloading" ? sum + download.bytes_per_second : sum),
    0,
  );

  const visible = downloads.filter((download) => {
    if (!matchesView(view, download)) return false;
    return matchesQuery(download, query);
  });
  const searching = query.trim() !== "";

  const summary = (() => {
    if (searching) {
      return `${plural(visible.length, "match", "matches")} for “${query.trim()}”`;
    }
    switch (view) {
      case "all":
        return [
          plural(downloads.length, "download"),
          moving > 0 ? `${moving} moving` : null,
          waiting > 0 ? `${waiting} waiting` : null,
        ]
          .filter(Boolean)
          .join(" · ");
      case "active":
        return [
          moving > 0 ? `${plural(moving, "transfer")} moving` : "Nothing moving",
          waiting > 0 ? `${waiting} waiting` : null,
          throughput > 0 ? formatSpeed(throughput) : null,
        ]
          .filter(Boolean)
          .join(" · ");
      case "queued":
        return waiting > 0
          ? `${plural(waiting, "download")} waiting for a free slot`
          : "Every slot is free";
      case "completed":
        return [
          plural(completed.length, "file"),
          completed.length > 0
            ? formatBytes(completed.reduce((sum, download) => sum + (download.total_bytes ?? 0), 0))
            : null,
        ]
          .filter(Boolean)
          .join(" · ");
      case "failed":
        return failed > 0 ? `${plural(failed, "transfer")} stopped · safe progress kept` : "No transfer has failed";
      case "history":
        return `${plural(terminal, "entry", "entries")} in the archive`;
    }
  })();

  return (
    <header className="shrink-0 border-b border-line-soft bg-background px-5 pt-5 pb-4 sm:px-6">
      <div className="flex flex-wrap items-start gap-x-5 gap-y-3">
        <div className="min-w-0 flex-1">
          <div className="eyebrow">{groupFor(view)}</div>
          <h1 className="mt-1.5 font-display text-[27px] font-semibold leading-none tracking-[-0.035em] text-ink-strong">
            {VIEW_COPY[view].title}
          </h1>
          <p className="mt-2 flex items-center gap-2 text-[11.5px] text-muted-foreground">
            <span
              aria-hidden="true"
              className={cn(
                "size-1.5 shrink-0 rounded-full",
                moving > 0 ? "breathe bg-brand" : "border border-faint",
              )}
            />
            <span className="truncate">{summary}</span>
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2 max-[900px]:w-full">
          <div className="relative w-[208px] max-[1200px]:w-[180px] max-[900px]:min-w-[150px] max-[900px]:flex-1">
            <Search
              className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-faint"
              strokeWidth={1.75}
              aria-hidden="true"
            />
            <Input
              ref={searchRef}
              type="text"
              role="searchbox"
              aria-label="Search downloads"
              placeholder="Search transfers"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  setQuery("");
                  event.currentTarget.blur();
                }
              }}
              className="h-8 pr-8 pl-8 text-[12px]"
            />
            {query ? (
              <button
                type="button"
                aria-label="Clear search"
                onClick={() => {
                  setQuery("");
                  searchRef.current?.focus();
                }}
                className="absolute top-1/2 right-1.5 flex size-5 -translate-y-1/2 items-center justify-center rounded-[4px] text-faint transition-colors hover:bg-raise hover:text-foreground"
              >
                <X className="size-3" strokeWidth={1.75} />
              </button>
            ) : (
              <Kbd className="pointer-events-none absolute top-1/2 right-1.5 -translate-y-1/2">/</Kbd>
            )}
          </div>

          <div className="flex items-center gap-0.5 rounded-md border border-line-soft bg-panel/60 p-0.5">
            <IconButton
              label="Resume paused"
              onClick={() => void resumeAll()}
              disabled={paused === 0}
              size="icon-sm"
            >
              <Play strokeWidth={1.75} />
            </IconButton>
            <IconButton
              label="Pause active"
              onClick={() => void pauseAll()}
              disabled={pausable === 0}
              size="icon-sm"
            >
              <Pause strokeWidth={1.75} />
            </IconButton>
            <IconButton
              label="Stop all"
              onClick={() => void stopAll()}
              disabled={stoppable === 0}
              size="icon-sm"
              className="hover:text-danger"
            >
              <X strokeWidth={1.75} />
            </IconButton>
          </div>

          <IconButton
            label="Toggle inspector"
            shortcut="D"
            onClick={toggleDetails}
            active={detailsOpen}
            size="icon-sm"
            className="border border-line-soft bg-panel/60"
          >
            <PanelRight strokeWidth={1.75} />
          </IconButton>

          <Button onClick={onAdd} className="ml-0.5">
            <Plus strokeWidth={2} />
            Add download
          </Button>
        </div>
      </div>
    </header>
  );
}
