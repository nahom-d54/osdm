import type { EngineBridge, EngineErrorListener, SnapshotListener } from "@/lib/bridge/types";
import type { Download, EngineConfig } from "@/lib/types";
import { suggestFilename } from "@/lib/format";

/**
 * Thin adapter over the Tauri commands and events defined in
 * docs/bridge-contract.md. All command names are snake_case; payloads
 * mirror the engine's serde types exactly.
 */
export class TauriBridge implements EngineBridge {
  readonly kind = "tauri" as const;

  private async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(command, args);
  }

  async open(config: EngineConfig): Promise<void> {
    await this.invoke("engine_open", { config });
  }

  async shutdown(): Promise<void> {
    await this.invoke("engine_shutdown");
  }

  async enqueue(request: { url: string; destination: string }): Promise<string> {
    return this.invoke<string>("engine_enqueue", { request });
  }

  async pause(id: string): Promise<void> {
    await this.invoke("engine_pause", { id });
  }

  async resume(id: string): Promise<void> {
    await this.invoke("engine_resume", { id });
  }

  async retry(id: string): Promise<void> {
    await this.invoke("engine_retry", { id });
  }

  async cancel(id: string): Promise<void> {
    await this.invoke("engine_cancel", { id });
  }

  async reorder(ids: string[]): Promise<void> {
    await this.invoke("engine_reorder", { ids });
  }

  async setSpeedLimit(bytesPerSecond: number): Promise<void> {
    await this.invoke("engine_set_speed_limit", { bytesPerSecond });
  }

  async snapshot(): Promise<Download[]> {
    return this.invoke<Download[]>("engine_snapshot");
  }

  subscribe(listener: SnapshotListener): () => void {
    let unlisten: (() => void) | null = null;
    void import("@tauri-apps/api/event").then(async ({ listen }) => {
      const stop = await listen<Download[]>("engine://snapshot", (event) => listener(event.payload));
      // Engine instructions: on lag, refresh with snapshot().
      const interval = setInterval(() => {
        void this.snapshot().then(listener).catch(() => {});
      }, 5000);
      unlisten = () => {
        stop();
        clearInterval(interval);
      };
    });
    return () => unlisten?.();
  }

  onError(listener: EngineErrorListener): () => void {
    let unlisten: (() => void) | null = null;
    void import("@tauri-apps/api/event").then(async ({ listen }) => {
      const stop = await listen<string>("engine://error", (event) => listener(event.payload));
      unlisten = () => stop();
    });
    return () => unlisten?.();
  }

  // --- P1 engine additions (pending, see docs/bridge-contract.md) ---

  async removeDownload(id: string, removePartialFile: boolean): Promise<void> {
    await this.invoke("engine_remove_download", { id, removePartialFile });
  }

  async cleanupPartialData(): Promise<number> {
    return this.invoke<number>("engine_cleanup_partial_data");
  }

  // --- desktop workflows (Tauri side assumed available) ---

  async pickDirectory(title: string, defaultPath?: string): Promise<string | null> {
    return this.invoke<string | null>("pick_directory", { title, defaultPath: defaultPath ?? null });
  }

  async suggestFilename(url: string): Promise<string> {
    return suggestFilename(url);
  }

  async pathExists(path: string): Promise<boolean> {
    return this.invoke<boolean>("path_exists", { path });
  }

  async openPath(path: string): Promise<void> {
    await this.invoke("open_path", { path });
  }

  async revealPath(path: string): Promise<void> {
    await this.invoke("reveal_path", { path });
  }
}
