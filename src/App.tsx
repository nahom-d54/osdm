import { useEffect, useMemo, useRef, useState } from "react";
import { CircleAlert, X } from "lucide-react";

import { AddDownloadDialog } from "@/components/AddDownloadDialog";
import { AutomationDialog } from "@/components/AutomationDialog";
import { MediaDialog } from "@/components/MediaDialog";
import { SiteDialog } from "@/components/SiteDialog";
import { getBridge } from "@/lib/bridge";
import type { MediaOffer } from "@/lib/retrieval";
import { PowerCountdown } from "@/components/PowerCountdown";
import { captureUrls } from "@/lib/capture";
import { BrowserCaptureDialog } from "@/components/BrowserCaptureDialog";
import type { Download } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { DetailsPanel } from "@/components/DetailsPanel";
import { DesktopDialogs } from "@/components/DesktopDialogs";
import { OnboardingDialog } from "@/components/OnboardingDialog";
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
  const [automationOpen, setAutomationOpen] = useState(false);
  const [mediaOpen, setMediaOpen] = useState(false);
  const [siteOpen, setSiteOpen] = useState(false);
  const [mediaOffer, setMediaOffer] = useState<MediaOffer | null>(null);
  const [mediaOffers, setMediaOffers] = useState<MediaOffer[]>([]);
  const [capturedText, setCapturedText] = useState("");
  const [browserCaptures, setBrowserCaptures] = useState<Download[]>([]);
  const reviewedCaptures = useRef(new Set<string>());
  const [watchClipboard, setWatchClipboard] = useState(false);
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
    let disposed = false;
    let pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try { const captures = await getBridge().captureReviews(); if (!disposed) setBrowserCaptures(captures.filter((capture) => !reviewedCaptures.current.has(capture.id))); }
      catch { /* Startup/disconnection never invents a capture. */ }
      finally { pending = false; }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 1000);
    return () => { disposed = true; clearInterval(timer); };
  }, []);

  useEffect(() => {
    let disposed = false;
    let pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try { const offers = await getBridge().mediaOffers(); if (!disposed) setMediaOffers(offers); }
      catch { /* A disconnected desktop bridge must not manufacture offers. */ }
      finally { pending = false; }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 2000);
    return () => { disposed = true; clearInterval(timer); };
  }, []);

  useEffect(() => {
    let lastClipboard = "";
    let disposed = false;
    const offer = (text: string) => {
      try { if (captureUrls(text).length) { setCapturedText(text); setAutomationOpen(true); } } catch { /* non-link clipboard content is never captured */ }
    };
    const onDrop = (event: DragEvent) => {
      if (document.querySelector('[data-slot="dialog-content"]')) return;
      if ((event.target as HTMLElement | null)?.closest('input, textarea, [contenteditable="true"]')) return;
      if (event.dataTransfer?.types.includes("text/uri-list")) event.preventDefault();
      const text = event.dataTransfer?.getData("text/uri-list") || event.dataTransfer?.getData("text/plain") || "";
      try { if (captureUrls(text).length) { event.preventDefault(); offer(text); } } catch { /* not a URL drop */ }
    };
    const onDrag = (event: DragEvent) => {
      if ((event.target as HTMLElement | null)?.closest('input, textarea, [contenteditable="true"]')) return;
      if (event.dataTransfer?.types.some((type) => type === "text/uri-list" || type === "text/plain")) event.preventDefault();
    };
    const onFocus = () => {
      if (!watchClipboard || document.querySelector('[data-slot="dialog-content"]')) return;
      void navigator.clipboard.readText().then((text) => { if (!disposed && text !== lastClipboard) { lastClipboard = text; offer(text); } }).catch(() => {});
    };
    window.addEventListener("drop", onDrop); window.addEventListener("dragover", onDrag); window.addEventListener("focus", onFocus);
    return () => { disposed = true; window.removeEventListener("drop", onDrop); window.removeEventListener("dragover", onDrag); window.removeEventListener("focus", onFocus); };
  }, [watchClipboard]);

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
          <div className="flex shrink-0 flex-wrap items-center gap-3 border-b border-line-soft px-5 py-2 text-xs sm:px-6"><Button size="sm" variant="outline" onClick={() => { setCapturedText(""); setAutomationOpen(true); }}>Capture & queues</Button><Button size="sm" variant="outline" onClick={() => { setMediaOffer(null); setMediaOpen(true); }}>Save media</Button><Button size="sm" variant="outline" onClick={() => setSiteOpen(true)}>Site Grabber</Button><label className="flex items-center gap-2 text-muted-foreground"><input type="checkbox" checked={watchClipboard} onChange={(e) => setWatchClipboard(e.target.checked)} />Offer clipboard links on focus · this session</label></div>
          {mediaOffers.length > 0 && <div role="status" className="flex shrink-0 flex-wrap items-center gap-3 border-b border-brand-line bg-brand-soft px-5 py-2 text-xs sm:px-6"><span className="min-w-0 flex-1 truncate">Browser media: {mediaOffers[0].title || "Untitled media"}{mediaOffers.length > 1 ? ` · ${mediaOffers.length} offers` : ""}</span><Button size="sm" variant="secondary" onClick={() => { setMediaOffer(mediaOffers[0]); setMediaOpen(true); }}>Choose quality & destination</Button><Button size="sm" variant="ghost" onClick={() => { const id = mediaOffers[0].id; void getBridge().dismissMediaOffer(id).then(() => setMediaOffers((offers) => offers.filter((offer) => offer.id !== id))).catch(() => {}); }}>Dismiss</Button></div>}

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
        <AutomationDialog open={automationOpen} onClose={() => setAutomationOpen(false)} initialText={capturedText} />
        {browserCaptures[0] && <BrowserCaptureDialog key={browserCaptures[0].id} download={browserCaptures[0]} onReviewed={(id) => { reviewedCaptures.current.add(id); if (reviewedCaptures.current.size > 512) reviewedCaptures.current.delete(reviewedCaptures.current.values().next().value!); setBrowserCaptures((captures) => captures.filter((capture) => capture.id !== id)); }} />}
        <MediaDialog open={mediaOpen} onClose={() => setMediaOpen(false)} offer={mediaOffer} />
        <SiteDialog open={siteOpen} onClose={() => setSiteOpen(false)} />
        <SettingsDialog open={settingsOpen} onClose={() => setSettingsOpen(false)} />
        <DesktopDialogs />
        <OnboardingDialog />
        <PowerCountdown />
      </div>
    </TooltipProvider>
  );
}
