import { useLayoutEffect, useMemo, useRef } from "react";
import type { KeyboardEvent } from "react";
import { Layers, Plus, X } from "lucide-react";

import { DownloadRow } from "@/components/DownloadRow";
import { EmptyState } from "@/components/EmptyState";
import { Button } from "@/components/ui/button";
import { VIEW_COPY, matchesQuery, matchesView } from "@/lib/views";
import { useEngine } from "@/store/engine";

export function DownloadList({ onAdd }: { onAdd: () => void }) {
  const { downloads, view, query, selectedId, setSelected, setQuery, setView } = useEngine();
  const listRef = useRef<HTMLUListElement>(null);

  const visible = useMemo(() => {
    return downloads.filter(
      (download) => matchesView(view, download) && matchesQuery(download, query),
    );
  }, [downloads, view, query]);

  const onKey = (event: KeyboardEvent) => {
    const rows = Array.from(listRef.current?.querySelectorAll<HTMLLIElement>("li[data-row]") ?? []);
    if (rows.length === 0) return;
    const currentIndex = rows.findIndex((node) => node === document.activeElement);
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
    if (!selected || !matchesView(view, selected) || !matchesQuery(selected, query)) {
      setSelected(null);
    }
  }, [downloads, selectedId, view, query, setSelected]);

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
      <div aria-hidden="true" className="flex shrink-0 items-center gap-3 border-b border-line-soft px-4 py-2">
        <span className="w-9 shrink-0" />
        <span className="eyebrow flex-1">File</span>
        <span className="eyebrow w-[92px] shrink-0">State</span>
        <span className="eyebrow hidden w-[76px] shrink-0 text-right @min-[760px]:block">Rate</span>
        <span className="eyebrow hidden w-[62px] shrink-0 text-right @min-[920px]:block">Left</span>
        <span className="eyebrow w-[70px] shrink-0 text-right">Size</span>
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
          />
        ))}
      </ul>
    </div>
  );
}
