import { create } from "zustand";
import type { Download, EngineConfig } from "@/lib/types";
import { ACTIVE_STATES } from "@/lib/types";
import type { EngineBridge } from "@/lib/bridge/types";
import { getBridge } from "@/lib/bridge";
import { runBulk } from "@/lib/bulk";
import { useSettings } from "@/store/settings";
import type { AppSettings } from "@/store/settings";

export type ViewFilter = "all" | "unfinished" | "active" | "queued" | "completed" | "failed" | "history";

export interface EngineStore {
  ready: boolean;
  downloads: Download[];
  view: ViewFilter;
  query: string;
  categoryFilter: string | null;
  selectedId: string | null;
  checkedIds: string[];
  detailsOpen: boolean;
  banner: string | null;
  engineError: string | null;

  init(): Promise<void>;
  enqueue(url: string, destination: string, expectedSha256?: string, startPaused?: boolean, properties?: import("@/lib/types").DownloadProperties, credentials?: import("@/lib/types").BasicCredentials): Promise<void>;
  pause(id: string): Promise<void>;
  resume(id: string): Promise<void>;
  retry(id: string): Promise<void>;
  updateUrl(id: string, url: string): Promise<void>;
  cancel(id: string): Promise<void>;
  removeDownload(id: string, removePartialFile: boolean): Promise<void>;
  openFile(id: string): Promise<void>;
  revealFile(id: string): Promise<void>;
  move(id: string, direction: "up" | "down"): Promise<void>;
  pauseAll(): Promise<void>;
  resumeAll(): Promise<void>;
  stopAll(): Promise<void>;
  removeCompleted(): Promise<void>;
  removeChecked(removePartialFile: boolean): Promise<void>;
  setChecked(ids: string[]): void;
  applySpeedLimit(bytesPerSecond: number): Promise<void>;

  setView(view: ViewFilter): void;
  setQuery(query: string): void;
  setCategoryFilter(category: string | null): void;
  setSelected(id: string | null): void;
  toggleDetails(): void;
  clearBanner(): void;
}

function configFrom(settings: AppSettings): EngineConfig {
  return {
    max_active_downloads: settings.max_active_downloads,
    connections_per_download: settings.connections_per_download,
    max_connections: settings.max_connections,
    max_connections_per_host: settings.max_connections_per_host,
    split_threshold: settings.split_threshold_mib * 1024 * 1024,
    bytes_per_second: settings.speed_limit_bytes_per_second,
    max_retries: settings.max_retries,
  };
}

export const useEngine = create<EngineStore>((set, get) => {
  let bridge: EngineBridge | null = null;
  let initializing: Promise<void> | null = null;

  const runCommand = async (action: (b: EngineBridge) => Promise<unknown>): Promise<void> => {
    const b = getBridge();
    try {
      await action(b);
      set({ downloads: await b.snapshot() });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      set({ banner: message });
      // Bulk commands can partially succeed; always reconcile authoritative state.
      try { set({ downloads: await b.snapshot() }); } catch { /* retain last snapshot */ }
    }
  };

  return {
    ready: false,
    downloads: [],
    view: "all",
    query: "",
    categoryFilter: null,
    selectedId: null,
    checkedIds: [],
    detailsOpen: false,
    banner: null,
    engineError: null,

    async init() {
      if (bridge) return; // StrictMode double-mounts effects; init once.
      if (!initializing) {
        initializing = (async () => {
          const b = getBridge();
          const settings = useSettings.getState().settings;
          await b.open(configFrom(settings));
          const downloads = await b.snapshot();
          b.onError((message) => set({ engineError: message }));
          b.subscribe((downloads) => set({ downloads }));
          bridge = b;
          set({ ready: true, downloads, engineError: null });
        })().catch((error: unknown) => {
          set({ engineError: error instanceof Error ? error.message : String(error) });
        }).finally(() => { initializing = null; });
      }
      await initializing;
    },

    async enqueue(url, destination, expectedSha256, startPaused = false, properties, credentials) {
      // Let the add flow show engine rejections in its own surface instead of
      // closing optimistically and only raising a background banner.
      const b = getBridge();
      await b.enqueue({ url, destination, expected_sha256: expectedSha256 || null }, startPaused, properties, credentials);
      // Admission is already durable. A refresh failure must not invite a retry
      // that queues the same user intent twice.
      try { set({ downloads: await b.snapshot() }); }
      catch { set({ banner: "Download saved, but the list could not be refreshed. Wait for the next engine snapshot." }); }
    },

    pause: (id) => runCommand((b) => b.pause(id)),
    resume: (id) => runCommand((b) => b.resume(id)),
    retry: (id) => runCommand((b) => b.retry(id)),
    async updateUrl(id, url) {
      const b = getBridge();
      await b.updateUrl(id, url);
      set({ downloads: await b.snapshot() });
    },

    cancel: async (id) => {
      // Sends cancel including queued/paused; complements per-row pause.
      await runCommand((b) => b.cancel(id));
    },

    removeDownload: (id, removePartialFile) => {
      const { selectedId } = get();
      set({ selectedId: selectedId === id ? null : selectedId });
      return runCommand((b) => b.removeDownload(id, removePartialFile));
    },

    openFile: async (id) => {
      const download = get().downloads.find((d) => d.id === id);
      if (download?.state === "Completed") {
        await runCommand((b) => b.openPath(download.destination));
      }
    },

    revealFile: async (id) => {
      const download = get().downloads.find((d) => d.id === id);
      if (download?.state === "Completed") {
        await runCommand((b) => b.revealPath(download.destination));
      }
    },

    move: async (id, direction) => {
      const { downloads } = get();
      const queue = downloads.filter((d) => d.state === "Queued" || d.state === "Paused").map((d) => d.id);
      const index = queue.indexOf(id);
      const swapWith = direction === "up" ? queue[index - 1] : queue[index + 1];
      if (index < 0 || !swapWith) return;
      const next = downloads.map((d) => d.id);
      const a = next.indexOf(id);
      const bIndex = next.indexOf(swapWith);
      next[a] = swapWith;
      next[bIndex] = id;
      await runCommand((b) => b.reorder(next));
    },

    pauseAll: () => {
      const { downloads } = get();
      return runCommandWithAll(downloads.filter((d) => d.state === "Downloading" || d.state === "Queued").map((d) => d.id), (b, id) => b.pause(id));
    },

    resumeAll: () => {
      const { downloads } = get();
      return runCommandWithAll(downloads.filter((d) => d.state === "Paused").map((d) => d.id), (b, id) => b.resume(id));
    },

    stopAll: () => {
      const { downloads } = get();
      return runCommandWithAll(downloads.filter((d) => ACTIVE_STATES.includes(d.state)).map((d) => d.id), (b, id) => b.cancel(id));
    },

    removeCompleted: () => runCommandWithAll(
      get().downloads.filter((download) => download.state === "Completed").map((download) => download.id),
      (b, id) => b.removeDownload(id, false),
    ),
    removeChecked: async (removePartialFile) => {
      await runCommandWithAll(get().checkedIds, (b, id) => b.removeDownload(id, removePartialFile));
      set({ checkedIds: get().checkedIds.filter((id) => get().downloads.some((download) => download.id === id)) });
    },
    setChecked: (checkedIds) => set({ checkedIds }),

    applySpeedLimit: (bytesPerSecond) => runCommand((b) => b.setSpeedLimit(bytesPerSecond)),

    setView: (view) => set({ view, categoryFilter: null }),
    setCategoryFilter: (categoryFilter) => set({ categoryFilter, view: "all", selectedId: null }),
    setQuery: (query) => set({ query }),
    setSelected: (selectedId) => set({ selectedId, detailsOpen: selectedId != null }),
    toggleDetails: () => set((s) => ({ detailsOpen: !s.detailsOpen })),
    clearBanner: () => set({ banner: null, engineError: null }),
  };

  async function runCommandWithAll(ids: string[], action: (b: EngineBridge, id: string) => Promise<unknown>): Promise<void> {
    await runCommand(async (b) => {
      await runBulk(ids, (id) => action(b, id));
    });
  }
});
