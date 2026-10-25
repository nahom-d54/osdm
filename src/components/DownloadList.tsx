import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { Layers, Plus, X } from "lucide-react";

import { DownloadRow } from "@/components/DownloadRow";
import { EmptyState } from "@/components/EmptyState";
import { Button } from "@/components/ui/button";
import { VIEW_COPY, matchesQuery, matchesView } from "@/lib/views";
import { useEngine } from "@/store/engine";
import { orderDownloads, type DownloadSort } from "@/lib/download-order";
import { useSettings } from "@/store/settings";

export function DownloadList({ onAdd }: { onAdd: () => void }) {
  const { downloads, view, query, categoryFilter, queueFilter, automation, selectedId, setSelected, setQuery, setView, checkedIds, setChecked } = useEngine();
  const listRef = useRef<HTMLUListElement>(null);
  const columns = useSettings((state) => state.settings.visible_columns);
  const [sort, setSort] = useState<DownloadSort>("queue");
  const [descending, setDescending] = useState(false);
  const sortBy = (next: DownloadSort) => {
    setDescending(next === sort ? !descending : false);
    setSort(next);
  };
  const heading = (key: DownloadSort, label: string) => (
    <button type="button" onClick={() => sortBy(key)} aria-label={`Sort by ${label}${sort === key ? (descending ? ", descending" : ", ascending") : ""}`}>
      {label}{sort === key ? (descending ? " ↓" : " ↑") : ""}
    </button>
  );

  const visible = useMemo(() => {
    return orderDownloads(downloads.filter(
      (download) => matchesView(view, download) && matchesQuery(download, query) && (categoryFilter == null || (download.properties?.category_id ?? "") === categoryFilter) && (queueFilter == null || (automation.membership[download.id] ?? "") === queueFilter),
    ), sort, descending);
  }, [downloads, view, query, categoryFilter, queueFilter, automation, sort, descending]);

  const onKey = (event: KeyboardEvent) => {
    const rows = Array.from(listRef.current?.querySelectorAll<HTMLLIElement>("li[data-row]") ?? []);
    if (rows.length === 0) return;
    const currentIndex = rows.findIndex((node) => node === document.activeElement);
    if (currentIndex < 0) return;
    let target: number | null = null;
    if (event.key === "ArrowDown") target = Math.min(rows.length - 1, currentIndex + 1);
    if (event.key === "ArrowUp") target = Math.max(0, currentIndex - 1);
    if (target == null) return;
    event.preventDefault();
    rows[target]?.focus();
    const id = rows[target]?.dataset.row;
    if (id) setSelected(id);
  };

  // Keep selection honest when a filter, search, or deletion removes the row.
  useLayoutEffect(() => {
    if (!selectedId) return;
    const selected = downloads.find((download) => download.id === selectedId);
    if (!selected || !matchesView(view, selected) || !matchesQuery(selected, query) || !visible.some((item) => item.id === selectedId)) {
      setSelected(null);
    }
  }, [downloads, selectedId, view, query, setSelected, visible]);

  const searching = query.trim() !== "";

  if (visible.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center overflow-y-auto p-8">
        {searching ? (
          <EmptyState
            title="No matches"
            body={`Nothing in ${VIEW_COPY[view].label.toLowerCase()} matches “${query.trim()}”. Try a filename, domain, or full URL.`}
            action={
              <Button variant="secondary" onClick={() => setQuery("")}>
                <X />
                Clear search
              </Button>
            }
          />
        ) : (
          <EmptyState
            title={VIEW_COPY[view].emptyTitle}
            body={VIEW_COPY[view].emptyBody}
            action={
              view === "completed" || view === "failed" || view === "history" ? (
                <Button variant="secondary" onClick={() => setView("all")}>
                  <Layers />
                  View all downloads
                </Button>
              ) : (
                <Button onClick={onAdd}>
                  <Plus />
                  Add download
                </Button>
              )
            }
          />
        )}
      </div>
    );
  }

  return (
    <div className="@container flex min-h-0 min-w-0 flex-1 flex-col bg-background">
      <div className="flex shrink-0 items-center gap-3 border-b border-line-soft px-4 py-2">
        <input type="checkbox" aria-label="Select all visible downloads" checked={visible.length > 0 && visible.every((download) => checkedIds.includes(download.id))} onChange={(event) => setChecked(event.target.checked ? [...new Set([...checkedIds, ...visible.map((download) => download.id)])] : checkedIds.filter((id) => !visible.some((download) => download.id === id)))} />
        <button type="button" aria-label="Restore queue order" title="Queue order" className="w-9 shrink-0 text-xs" onClick={() => setSort("queue")}>#</button>
        <span className="eyebrow flex-1">{heading("file", "File")}</span>
        {columns.includes("state") && <span className="eyebrow w-[92px] shrink-0">{heading("state", "State")}</span>}
        {columns.includes("rate") && <span className="eyebrow hidden w-[76px] shrink-0 text-right @min-[760px]:block">{heading("rate", "Rate")}</span>}
        {columns.includes("eta") && <span className="eyebrow hidden w-[62px] shrink-0 text-right @min-[920px]:block">{heading("eta", "Left")}</span>}
        {columns.includes("size") && <span className="eyebrow w-[70px] shrink-0 text-right">{heading("size", "Size")}</span>}
        <span className="w-[56px] shrink-0" />
      </div>

      <ul
        ref={listRef}
        onKeyDown={onKey}
        aria-label="Downloads"
        className="min-h-0 min-w-0 flex-1 overflow-y-auto"
      >
        {visible.map((download, index) => (
          <DownloadRow
            key={download.id}
            download={download}
            index={index}
            selected={download.id === selectedId}
            tabStop={download.id === selectedId || (selectedId == null && index === 0)}
          />
        ))}
      </ul>
    </div>
  );
}
