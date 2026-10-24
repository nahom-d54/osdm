import { useEffect, useMemo, useRef, useState } from "react";
import { Sidebar } from "@/components/Sidebar";
import { Toolbar } from "@/components/Toolbar";
import { DownloadList } from "@/components/DownloadList";
import { DetailsPanel } from "@/components/DetailsPanel";
import { StatusBar } from "@/components/StatusBar";
import { AddDownloadDialog } from "@/components/AddDownloadDialog";
import { SettingsDialog } from "@/components/SettingsDialog";
import { useEngine } from "@/store/engine";
import { useSettings } from "@/store/settings";
import { XIcon } from "@/components/icons";

export default function App() {
  const { init, selectedId, downloads, banner, engineError, clearBanner, toggleDetails, detailsOpen } = useEngine();
  const settings = useSettings((s) => s.settings);
  const [addOpen, setAddOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);

  // Theme: system choice follows the OS via media query.
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
      const typing = target?.tagName === "INPUT" || target?.tagName === "TEXTAREA" || target?.tagName === "SELECT";
      if (typing) return;
      if (event.key === "n" || (event.ctrlKey && event.key === "n")) {
        event.preventDefault();
        setAddOpen(true);
      } else if (event.key === "/" || (event.ctrlKey && event.key === "f")) {
        event.preventDefault();
        searchRef.current?.focus();
      } else if (event.key === "d") {
        toggleDetails();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggleDetails]);

  const selected = useMemo(
    () => downloads.find((d) => d.id === selectedId) ?? null,
    [downloads, selectedId],
  );

  const notice = engineError ?? banner;

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <Toolbar searchRef={searchRef} onAdd={() => setAddOpen(true)} onSettings={() => setSettingsOpen(true)} />

        {notice && (
          <div role="alert" className="flex items-center justify-between gap-3 border-b border-danger/30 bg-danger-soft px-4 py-2 text-[12px] text-danger">
            <span>
              {notice}
              <span className="ms-2 text-danger/70">The engine keeps running; nothing was deleted.</span>
            </span>
            <button type="button" aria-label="Dismiss" onClick={clearBanner} className="text-danger/70 hover:text-danger">
              <XIcon size={12} />
            </button>
          </div>
        )}

        <div className="flex min-h-0 flex-1">
          <DownloadList />
          {detailsOpen && <DetailsPanel download={selected} />}
        </div>

        <StatusBar />
      </div>

      <AddDownloadDialog open={addOpen} onClose={() => setAddOpen(false)} />
      <SettingsDialog open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}
