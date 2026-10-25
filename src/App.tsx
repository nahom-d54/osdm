import { useEffect, useMemo, useRef, useState } from "react";
import { CircleAlert, X } from "lucide-react";

import { AddDownloadDialog } from "@/components/AddDownloadDialog";
import { DetailsPanel } from "@/components/DetailsPanel";
import { DownloadList } from "@/components/DownloadList";
import { SettingsDialog } from "@/components/SettingsDialog";
import { Sidebar } from "@/components/Sidebar";
import { StatusBar } from "@/components/StatusBar";
import { Toolbar } from "@/components/Toolbar";
import { IconButton } from "@/components/ui/icon-button";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useEngine } from "@/store/engine";
import { useSettings } from "@/store/settings";

export default function App() {
  const {
    init,
    selectedId,
    downloads,
    banner,
    engineError,
    clearBanner,
    toggleDetails,
    detailsOpen,
  } = useEngine();
  const settings = useSettings((state) => state.settings);
  const [addOpen, setAddOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);

  // Theme: "system" follows the OS through a media query.
  useEffect(() => {
    const root = document.documentElement;
    const apply = () => {
      const resolved =
        settings.theme === "system"
          ? window.matchMedia("(prefers-color-scheme: dark)").matches
            ? "dark"
            : "light"
          : settings.theme;
      root.dataset.theme = resolved;
    };
    apply();
    if (settings.theme !== "system") return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [settings.theme]);

  useEffect(() => {
    void init();
  }, [init]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing =
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.tagName === "SELECT" ||
        target?.isContentEditable;
      if (typing) return;
      if (
        addOpen ||
        settingsOpen ||
        document.querySelector('[data-slot="dialog-content"]') != null
      ) {
        return;
      }

      if (event.key === "n" || (event.ctrlKey && event.key === "n")) {
        event.preventDefault();
        setAddOpen(true);
      } else if (event.key === "/" || (event.ctrlKey && event.key === "f")) {
        event.preventDefault();
        searchRef.current?.focus();
      } else if (event.key === "d" && !event.ctrlKey && !event.metaKey && !event.altKey) {
        toggleDetails();
      } else if (event.key === "Escape" && detailsOpen) {
        toggleDetails();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggleDetails, addOpen, settingsOpen, detailsOpen]);

  const selected = useMemo(
    () => downloads.find((download) => download.id === selectedId) ?? null,
    [downloads, selectedId],
  );

  const notice = engineError ?? banner;

  return (
    <TooltipProvider>
      <div className="flex h-screen overflow-hidden bg-background text-foreground">
        <Sidebar onSettings={() => setSettingsOpen(true)} />

        <div className="flex min-w-0 flex-1 flex-col">
          <Toolbar searchRef={searchRef} onAdd={() => setAddOpen(true)} />

          {notice && (
            <div
              role="alert"
              className="flex shrink-0 items-center gap-2 border-b border-danger/20 bg-danger-soft px-5 py-2 text-[11px] text-danger sm:px-6"
            >
              <CircleAlert className="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
              <span className="min-w-0 flex-1">
                {notice}
                <span className="ml-2 text-danger/65">
                  {engineError ? "The engine reported a fault." : "The action was not completed."}
                </span>
              </span>
              <IconButton label="Dismiss notice" onClick={clearBanner} size="icon-xs" className="text-danger hover:bg-danger/10">
                <X strokeWidth={1.75} />
              </IconButton>
            </div>
          )}

          <div className="relative flex min-h-0 flex-1">
            <DownloadList onAdd={() => setAddOpen(true)} />
            {detailsOpen && (
              <>
                {selected && (
                  <button
                    type="button"
                    tabIndex={-1}
                    aria-label="Close inspector"
                    onClick={toggleDetails}
                    className="absolute inset-0 z-30 cursor-default bg-black/30 backdrop-blur-[1px] min-[1320px]:hidden"
                  />
                )}
                <DetailsPanel download={selected} />
              </>
            )}
          </div>

          <StatusBar />
        </div>

        <AddDownloadDialog open={addOpen} onClose={() => setAddOpen(false)} />
        <SettingsDialog open={settingsOpen} onClose={() => setSettingsOpen(false)} />
      </div>
    </TooltipProvider>
  );
}
