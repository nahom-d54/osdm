import type { Download } from "@/lib/types";
import { useEngine } from "@/store/engine";
import type { ViewFilter } from "@/store/engine";

const GROUPS: Array<{ heading: string; items: Array<{ id: ViewFilter; label: string; match?: (d: Download) => boolean }> }> = [
  {
    heading: "Library",
    items: [{ id: "all", label: "All downloads" }],
  },
  {
    heading: "In progress",
    items: [
      { id: "active", label: "Active", match: (d) => ["Queued", "Downloading", "Pausing", "Paused", "Canceling"].includes(d.state) },
      { id: "queued", label: "Queued", match: (d) => d.state === "Queued" || d.state === "Paused" },
    ],
  },
  {
    heading: "Finished",
    items: [
      { id: "completed", label: "Completed", match: (d) => d.state === "Completed" },
      { id: "failed", label: "Failed", match: (d) => d.state === "Failed" },
      { id: "history", label: "History", match: (d) => ["Completed", "Failed", "Canceled"].includes(d.state) },
    ],
  },
];

export function Sidebar() {
  const { view, setView, downloads } = useEngine();

  const countFor = (match?: (d: Download) => boolean): number | null => {
    if (!match) return downloads.length;
    const n = downloads.filter(match).length;
    return n > 0 ? n : null;
  };

  return (
    <nav aria-label="Download views" className="flex w-52 shrink-0 flex-col gap-5 border-r border-line bg-panel px-3 py-4">
      <div className="px-2">
        <div className="font-display text-[17px] font-bold tracking-tight text-ink-strong">
          OSDM
        </div>
        <div className="mt-0.5 font-mono text-[10px] tracking-wide text-faint">
          0.1.0-alpha · local
        </div>
      </div>

      {GROUPS.map((group) => (
        <section key={group.heading} className="flex flex-col gap-0.5">
          <h3 className="px-2 pb-1 text-[10px] font-semibold tracking-[0.08em] text-faint uppercase">
            {group.heading}
          </h3>
          {group.items.map((item) => {
            const count = countFor(item.match);
            const selected = view === item.id;
            return (
              <button
                key={item.id}
                type="button"
                aria-current={selected ? "page" : undefined}
                onClick={() => setView(item.id)}
                className={`flex items-center justify-between rounded-[var(--radius-control)] px-2 py-1.5 text-left text-[13px] transition-colors ${
                  selected ? "bg-accent-soft font-medium text-accent" : "text-muted hover:bg-raise hover:text-ink"
                }`}
              >
                <span className="truncate">{item.label}</span>
                {count != null && <span className="font-mono text-[11px] text-faint">{count}</span>}
              </button>
            );
          })}
        </section>
      ))}

      <p className="mt-auto px-2 text-[11px] leading-relaxed text-faint">
        Categories, queues, and captured downloads arrive in later releases.
      </p>
    </nav>
  );
}

export function matchesView(view: ViewFilter, d: Download): boolean {
  switch (view) {
    case "all":
      return true;
    case "active":
      return ["Queued", "Downloading", "Pausing", "Paused", "Canceling"].includes(d.state);
    case "queued":
      return d.state === "Queued" || d.state === "Paused";
    case "completed":
      return d.state === "Completed";
    case "failed":
      return d.state === "Failed";
    case "history":
      return ["Completed", "Failed", "Canceled"].includes(d.state);
  }
}
