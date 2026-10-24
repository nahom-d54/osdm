import type { RefObject } from "react";
import { GearIcon, PauseIcon, PlayIcon, PlusIcon, SearchIcon, SidebarIcon, XIcon } from "@/components/icons";
import { Button, IconButton } from "@/components/ui/Button";
import { useEngine } from "@/store/engine";

export function Toolbar({ searchRef, onAdd, onSettings }: {
  searchRef: RefObject<HTMLInputElement | null>;
  onAdd: () => void;
  onSettings: () => void;
}) {
  const { pauseAll, resumeAll, stopAll, query, setQuery, detailsOpen, toggleDetails } = useEngine();

  return (
    <div className="flex h-12 shrink-0 items-center gap-2 border-b border-line bg-panel px-3">
      <Button variant="primary" onClick={onAdd} className="mr-1">
        <PlusIcon size={13} />
        Add download
      </Button>

      <div className="group flex overflow-hidden rounded-[var(--radius-control)] border border-line">
        <Button size="md" variant="ghost" onClick={resumeAll} title="Resume all paused downloads" className="rounded-none border-r border-line">
          <PlayIcon size={12} />
        </Button>
        <Button size="md" variant="ghost" onClick={pauseAll} title="Pause all active downloads" className="rounded-none">
          <PauseIcon size={12} />
        </Button>
      </div>
      <Button size="md" variant="ghost" onClick={stopAll} title="Stop all active downloads">
        <XIcon size={12} />
      </Button>

      <div className="relative ml-auto w-64">
        <SearchIcon size={13} className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-faint" />
        <input
          ref={searchRef}
          type="search"
          role="searchbox"
          aria-label="Search downloads"
          placeholder="Search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          className="h-8.5 w-full rounded-[var(--radius-control)] border border-line bg-raise ps-8 pe-8 text-[12.5px] text-ink placeholder:text-faint focus:outline-none focus-visible:border-accent"
        />
        <kbd className="pointer-events-none absolute top-1/2 end-2 -translate-y-1/2">/</kbd>
      </div>

      <IconButton
        label={detailsOpen ? "Hide details panel" : "Show details panel"}
        active={detailsOpen ? "bg-accent-soft text-accent opacity-100" : ""}
        onClick={toggleDetails}
      >
        <SidebarIcon size={13} />
      </IconButton>
      <IconButton label="Settings" onClick={onSettings}>
        <GearIcon size={13} />
      </IconButton>
    </div>
  );
}
