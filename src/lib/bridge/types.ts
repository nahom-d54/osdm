import type { Download } from "@/lib/types";

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
  pause(id: string): Promise<void>;
  resume(id: string): Promise<void>;
  retry(id: string): Promise<void>;
  cancel(id: string): Promise<void>;
  /** Full ordered list of every queue/history id, each exactly once. */
  reorder(ids: string[]): Promise<void>;
  /** Zero means unlimited. */
  setSpeedLimit(bytesPerSecond: number): Promise<void>;
  snapshot(): Promise<Download[]>;
  /** Full snapshots at up to 4 Hz; refresh with snapshot() after lag. */
  subscribe(listener: SnapshotListener): () => void;
  onError(listener: EngineErrorListener): () => void;

  // --- P1 engine additions (pending on the Rust side, see docs) ---
  /** Delete a download entry; optionally remove its temporary data. */
  removeDownload(id: string, removePartialFile: boolean): Promise<void>;
  /** Remove leftover temporary files that no download owns. */
  cleanupPartialData(): Promise<number>;

  // --- Desktop workflows (Tauri side assumed available) ---
  pickDirectory(title: string, defaultPath?: string): Promise<string | null>;
  suggestFilename(url: string): Promise<string>;
  pathExists(path: string): Promise<boolean>;
  openPath(path: string): Promise<void>;
  revealPath(path: string): Promise<void>;
}
