import { useLayoutEffect, useRef } from "react";
import type { KeyboardEvent } from "react";
import { useEngine } from "@/store/engine";
import { matchesView } from "@/components/Sidebar";
import { DownloadRow } from "@/components/DownloadRow";
import { EmptyState } from "@/components/EmptyState";
const EMPTY_COPY: Record<string, { title: string; body: string }> = {
  all: {
    title: "No downloads yet",
    body: "Press N or use Add download to paste a direct URL. The engine never overwrites an existing file.",
  },
  active: {
    title: "Nothing is running",
    body: "Queued and downloading items appear here as they work.",
  },
  queued: {
    title: "The queue is empty",
    body: "Downloads wait here when all connection slots are busy or were added with pause-for-now.",
  },
  completed: {
    title: "Nothing has finished",
    body: "Completed files stay here so you can open or locate them later.",
  },
  failed: {
    title: "Nothing failed",
    body: "Failed downloads keep their safe progress. Retry when the server is reachable again.",
  },
  history: {
    title: "History is empty",
    body: "Completed, failed, and stopped downloads appear here.",
  },
};

export function DownloadList() {
  const { downloads, view, query, selectedId, setSelected } = useEngine();
  const listRef = useRef<HTMLUListElement>(null);

  const visible = downloads.filter(
    (d) =>
      matchesView(view, d) &&
      (d.url.toLowerCase().includes(query.toLowerCase()) ||
        d.destination.toLowerCase().includes(query.toLowerCase())),
  );

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

  // After a delete, keep a sensible selection and view context.
  useLayoutEffect(() => {
    if (selectedId && !downloads.some((d) => d.id === selectedId)) {
      setSelected(null);
    }
  }, [downloads, selectedId, setSelected]);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col bg-bg">
      {visible.length === 0 ? (
        <div className="flex flex-1 items-center justify-center p-8">
          <EmptyState
            title={EMPTY_COPY[view]?.title ?? "Nothing here"}
            body={EMPTY_COPY[view]?.body ?? "Change the filter to see other downloads."}
          />
        </div>
      ) : (
        <ul
          ref={listRef}
          onKeyDown={onKey}
          aria-label="Downloads"
          className="flex flex-col divide-y divide-line-soft overflow-y-auto px-2 py-1.5"
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
      )}
    </div>
  );
}
