import type { ReactElement } from "react";
import { Activity, Archive, CircleAlert, CircleCheck, Clock, Layers, Settings2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { cn } from "cn";

import mark from "@/assets/osdm-mark.png";
import wordmark from "@/assets/osdm-wordmark.png";
import { ThroughputSpark } from "@/components/ThroughputSpark";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatSpeed } from "@/lib/format";
import { useMediaQuery } from "@/lib/use-media";
import { NAV_GROUPS, VIEW_COPY, matchesView } from "@/lib/views";
import type { ViewFilter } from "@/store/engine";
import { useEngine } from "@/store/engine";
import { useSettings } from "@/store/settings";

const VIEW_ICONS: Record<ViewFilter, LucideIcon> = {
  all: Layers,
  unfinished: Clock,
  active: Activity,
  queued: Clock,
  completed: CircleCheck,
  failed: CircleAlert,
  history: Archive,
};

export function Sidebar({ onSettings }: { onSettings: () => void }) {
  const { view, setView, downloads, categoryFilter, setCategoryFilter, queueFilter, setQueueFilter, automation } = useEngine();
  const categories = useSettings((state) => state.settings.categories);
  const compact = useMediaQuery("(max-width: 1023px)");

  const active = downloads.filter((download) => download.state === "Downloading").length;
  const waiting = downloads.filter((download) => download.state === "Queued" || download.state === "Paused").length;
  const throughput = downloads.reduce(
    (sum, download) => (download.state === "Downloading" ? sum + download.bytes_per_second : sum),
    0,
  );
  const [rawSpeed, speedUnit = "B/s"] = formatSpeed(throughput).split(" ");
  const speedValue =
    throughput > 0 ? (Number(rawSpeed) >= 100 ? rawSpeed : Number(rawSpeed).toFixed(1)) : "0";

  const countFor = (target: ViewFilter): number => {
    if (target === "all") return downloads.length;
    return downloads.filter((download) => matchesView(target, download)).length;
  };

  return (
    <nav
      aria-label="Download views"
      className="flex w-[236px] shrink-0 flex-col border-r border-line-soft bg-panel max-[1023px]:w-[64px]"
    >
      <div className="flex h-[76px] shrink-0 flex-col justify-center gap-1.5 px-5 max-[1023px]:h-[68px] max-[1023px]:items-center max-[1023px]:px-0">
        <img
          src={wordmark}
          alt="OSDM"
          width={640}
          height={154}
          draggable={false}
          className="h-auto w-[150px] select-none max-[1023px]:hidden"
        />
        <div className="font-mono text-[8px] uppercase leading-none tracking-[0.18em] text-faint max-[1023px]:hidden">
          Transfer desk
        </div>
        <img
          src={mark}
          alt="OSDM"
          width={512}
          height={512}
          draggable={false}
          className="hidden size-10 select-none max-[1023px]:block"
        />
      </div>

      <section
        aria-label="Aggregate throughput"
        className="mx-3 mt-1 rounded-lg border border-line-soft bg-raise/40 px-3 py-2.5 max-[1023px]:hidden"
      >
        <div className="flex items-center justify-between">
          <span className="eyebrow">Throughput</span>
          <span
            aria-hidden="true"
            className={cn("size-1.5 rounded-full", active > 0 ? "breathe bg-ok" : "border border-faint")}
          />
        </div>
        <div className="mt-1.5 flex items-baseline gap-1.5">
          <span className="font-display text-[23px] font-semibold leading-none tracking-[-0.04em] text-ink-strong tnum">
            {throughput > 0 ? speedValue : "0"}
          </span>
          <span className="font-mono text-[9px] uppercase tracking-[0.08em] text-faint">
            {throughput > 0 ? speedUnit : "B/s"}
          </span>
        </div>
        <div className="mt-2.5">
          <ThroughputSpark />
        </div>
        <div className="mt-2 font-mono text-[9px] uppercase tracking-[0.06em] text-faint tnum">
          {active} active · {waiting} waiting
        </div>
      </section>

      <div className="mt-3 min-h-0 flex-1 overflow-y-auto px-2.5 pb-2">
        {NAV_GROUPS.map((group) => (
          <section key={group.heading} className="mb-4 last:mb-0">
            <h2 className="eyebrow px-2 pb-1.5 max-[1023px]:hidden">{group.heading}</h2>
            <div className="flex flex-col gap-0.5">
              {group.views.map((target) => {
                const selected = view === target && categoryFilter == null && queueFilter == null;
                const count = countFor(target);
                const trigger = (
                  <button
                    type="button"
                    aria-label={`${VIEW_COPY[target].label}${count > 0 ? `, ${count}` : ""}`}
                    aria-current={selected ? "page" : undefined}
                    onClick={() => setView(target)}
                    className={cn(
                      "flex h-8 w-full items-center gap-2.5 rounded-md px-2 text-left text-[12.5px] transition-colors max-[1023px]:justify-center max-[1023px]:px-0",
                      selected
                        ? "bg-brand-soft font-medium text-brand"
                        : "text-muted-foreground hover:bg-raise hover:text-foreground",
                    )}
                  >
                    <NavIcon target={target} />
                    <span className="flex-1 truncate max-[1023px]:hidden">{VIEW_COPY[target].label}</span>
                    {count > 0 && (
                      <span
                        className={cn(
                          "font-mono text-[10px] tnum max-[1023px]:hidden",
                          selected ? "text-brand/80" : "text-faint",
                        )}
                      >
                        {count}
                      </span>
                    )}
                  </button>
                );
                return (
                  <Hint
                    key={target}
                    enabled={compact}
                    label={`${VIEW_COPY[target].label}${count > 0 ? ` · ${count}` : ""}`}
                  >
                    {trigger}
                  </Hint>
                );
              })}
            </div>
          </section>
        ))}
        <section aria-label="Categories" className="mb-4">
          <h2 className="eyebrow px-2 pb-1.5 max-[1023px]:hidden">Categories</h2>
          {[{ id: "", name: "General" }, ...categories, ...[...new Set(downloads.map((download) => download.properties?.category_id).filter((id): id is string => !!id && !categories.some((category) => category.id === id)))].map((id) => ({ id, name: "Deleted category" }))].map((category) => <button key={category.id} type="button" title={category.name} aria-label={`Category: ${category.name}`} aria-current={categoryFilter === category.id ? "page" : undefined} onClick={() => setCategoryFilter(category.id)} className={cn("flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left text-xs", categoryFilter === category.id ? "bg-brand-soft text-brand" : "text-muted-foreground hover:bg-raise")}>
            <span aria-hidden="true">#</span><span className="truncate max-[1023px]:hidden">{category.name}</span>
            <span className="ml-auto max-[1023px]:hidden">{downloads.filter((download) => (download.properties?.category_id ?? "") === category.id).length}</span>
          </button>)}
        </section>
        <section aria-label="Queues" className="mb-4"><h2 className="eyebrow px-2 pb-1.5 max-[1023px]:hidden">Queues</h2>{[{ id: "", name: "General queue" }, ...automation.queues].map((queue) => <button key={queue.id} type="button" aria-label={`Queue: ${queue.name}`} title={queue.name} aria-current={queueFilter === queue.id ? "page" : undefined} onClick={() => setQueueFilter(queue.id)} className={cn("flex min-h-8 w-full items-center gap-2 rounded-md px-2 text-left text-xs", queueFilter === queue.id ? "bg-brand-soft text-brand" : "text-muted-foreground hover:bg-raise")}><Clock className="size-3 shrink-0" aria-hidden="true" /><span className="truncate max-[1023px]:hidden">{queue.name}</span><span className="ml-auto max-[1023px]:hidden">{downloads.filter((d) => (automation.membership[d.id] ?? "") === queue.id).length}</span></button>)}</section>
      </div>

      <div className="shrink-0 border-t border-line-soft p-2.5">
        <Hint enabled={compact} label="Settings">
          <button
            type="button"
            aria-label="Settings"
            onClick={onSettings}
            className="flex h-8 w-full items-center gap-2.5 rounded-md px-2 text-[12.5px] text-muted-foreground transition-colors hover:bg-raise hover:text-foreground max-[1023px]:justify-center max-[1023px]:px-0"
          >
            <Settings2 className="size-[15px] shrink-0" strokeWidth={1.75} aria-hidden="true" />
            <span className="max-[1023px]:hidden">Settings</span>
          </button>
        </Hint>
        <div className="mt-1 px-2 font-mono text-[8.5px] uppercase tracking-[0.1em] text-faint max-[1023px]:hidden">
          OSDM · local
        </div>
      </div>
    </nav>
  );
}

function NavIcon({ target }: { target: ViewFilter }) {
  const Icon = VIEW_ICONS[target];
  return <Icon className="size-[15px] shrink-0" strokeWidth={1.75} aria-hidden="true" />;
}

function Hint({ enabled, label, children }: { enabled: boolean; label: string; children: ReactElement }) {
  if (!enabled) return children;
  return (
    <Tooltip>
      <TooltipTrigger render={children} />
      <TooltipContent side="right">{label}</TooltipContent>
    </Tooltip>
  );
}
