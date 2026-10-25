import type { EngineBridge, EngineErrorListener, SnapshotListener } from "@/lib/bridge/types";
import type { Download, EngineConfig, UrlProbe } from "@/lib/types";
import { suggestFilename } from "@/lib/format";
import { useSettings } from "@/store/settings";
import type { AppSettings } from "@/store/settings";

/**
 * Thin adapter over the Tauri commands and events defined in
 * docs/bridge-contract.md. All command names are snake_case; payloads
 * mirror the engine's serde types exactly.
 */
export class TauriBridge implements EngineBridge {
  readonly kind = "tauri" as const;
  private stopSettings: (() => void) | null = null;
  private settingsWrites: Promise<void> = Promise.resolve();
  private errorListeners = new Set<EngineErrorListener>();

  private report(error: unknown): void {
    const message = error instanceof Error ? error.message : String(error);
    this.errorListeners.forEach((listener) => listener(message));
  }

  private async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(command, args);
  }

  async open(config: EngineConfig): Promise<void> {
    const restored = await this.invoke<AppSettings | null>("settings_load");
    if (restored) useSettings.setState({ settings: restored });
    else await this.invoke("settings_save", { settings: useSettings.getState().settings });
    // Native settings are authoritative when present; engine_open reads the same
    // persisted config, so clearing webview storage cannot reset concurrency.
    await this.invoke("engine_open", { config });
    if (!this.stopSettings) {
      this.stopSettings = useSettings.subscribe((state, previous) => {
        if (state.settings === previous.settings) return;
        this.settingsWrites = this.settingsWrites.catch(() => {}).then(() =>
          this.invoke<void>("settings_save", { settings: state.settings }),
        );
        void this.settingsWrites.catch((error) => this.report(error));
      });
    }
  }

  async shutdown(): Promise<void> {
    await this.settingsWrites;
    await this.invoke("engine_shutdown");
    this.stopSettings?.();
    this.stopSettings = null;
  }

  async enqueue(request: { url: string; destination: string }): Promise<string> {
    return this.invoke<string>("engine_enqueue", { request });
  }

  async probe(url: string): Promise<UrlProbe> {
    return this.invoke<UrlProbe>("engine_probe", { url });
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
    let disposed = false;
    void import("@tauri-apps/api/event").then(async ({ listen }) => {
      const stop = await listen<Download[]>("engine://snapshot", (event) => listener(event.payload));
      if (disposed) { stop(); return; }
      // Engine instructions: on lag, refresh with snapshot().
      const interval = setInterval(() => {
        void this.snapshot().then((snapshot) => { if (!disposed) listener(snapshot); }).catch((error) => this.report(error));
      }, 5000);
      unlisten = () => {
        stop();
        clearInterval(interval);
      };
    }).catch((error) => this.report(error));
    return () => { disposed = true; unlisten?.(); };
  }

  onError(listener: EngineErrorListener): () => void {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    this.errorListeners.add(listener);
    void import("@tauri-apps/api/event").then(async ({ listen }) => {
      const stop = await listen<string>("engine://error", (event) => listener(event.payload));
      if (disposed) { stop(); return; }
      unlisten = () => stop();
    }).catch((error) => this.report(error));
    return () => { disposed = true; this.errorListeners.delete(listener); unlisten?.(); };
  }

  // --- P1 engine additions ---

  async removeDownload(id: string, removePartialFile: boolean): Promise<void> {
    await this.invoke("engine_remove_download", { id, removePartialFile });
  }

  async cleanupPartialData(): Promise<number> {
    return this.invoke<number>("engine_cleanup_partial_data");
  }

  // --- Native desktop workflows ---

  async pickDirectory(title: string, defaultPath?: string): Promise<string | null> {
    return this.invoke<string | null>("pick_directory", { title, defaultPath: defaultPath ?? null });
  }

  async defaultDownloadDirectory(): Promise<string> {
    return this.invoke<string>("default_download_directory");
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
