import type { Download, UrlProbe } from "@/lib/types";

export interface SnapshotListener {
  (downloads: Download[]): void;
}

export interface EngineErrorListener {
  (message: string): void;
}

/**
 * The only seam between the UI and the engine. The Tauri implementation
 * is a thin 1:1 mapping of the commands listed in docs/bridge-contract.md;
 * the mock implementation lets the shell run in a plain browser tab.
 */
export interface EngineBridge {
  kind: "mock" | "tauri";

  // --- Engine lifecycle (Engine::open / shutdown) ---
  open(config: import("@/lib/types").EngineConfig): Promise<void>;
  shutdown(): Promise<void>;

  // --- Engine commands (mirrored 1:1) ---
  enqueue(request: import("@/lib/types").DownloadRequest): Promise<string>;
  /** Read final URL, server filename, length, and range support without enqueueing. */
  probe(url: string): Promise<UrlProbe>;
  pause(id: string): Promise<void>;
  resume(id: string): Promise<void>;
  retry(id: string): Promise<void>;
  updateUrl(id: string, url: string): Promise<void>;
  /** Ask the native shell to check GitHub Releases now. */
  checkForUpdates(): Promise<void>;
  cancel(id: string): Promise<void>;
  /** Full ordered list of every queue/history id, each exactly once. */
  reorder(ids: string[]): Promise<void>;
  /** Zero means unlimited. */
  setSpeedLimit(bytesPerSecond: number): Promise<void>;
  snapshot(): Promise<Download[]>;
  /** Full snapshots at up to 4 Hz; refresh with snapshot() after lag. */
  subscribe(listener: SnapshotListener): () => void;
  onError(listener: EngineErrorListener): () => void;

  // --- P1 engine additions ---
  /** Delete a download entry; optionally remove its temporary data. */
  removeDownload(id: string, removePartialFile: boolean): Promise<void>;
  /** Remove Failed/Canceled partials and entryless partials in the engine ownership ledger. */
  cleanupPartialData(): Promise<number>;

  // --- Native desktop workflows ---
  pickDirectory(title: string, defaultPath?: string): Promise<string | null>;
  /** Existing per-user download folder used when no folder has been chosen yet. */
  defaultDownloadDirectory(): Promise<string>;
  suggestFilename(url: string): Promise<string>;
  pathExists(path: string): Promise<boolean>;
  openPath(path: string): Promise<void>;
  revealPath(path: string): Promise<void>;
}
