import type { EngineBridge, EngineErrorListener, SnapshotListener } from "@/lib/bridge/types";
import type { Download, DownloadRequest, DownloadProperties, EngineConfig, UrlProbe } from "@/lib/types";
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

  async enqueue(request: DownloadRequest, startPaused = false, properties?: DownloadProperties, credentials?: import("@/lib/types").BasicCredentials): Promise<string> {
    // Native admission reads working-storage preferences; never race a pending
    // settings write and silently use the previous directory.
    await this.settingsWrites;
    return this.invoke<string>("engine_enqueue", { request, startPaused, properties, credentials });
  }

  async setCredentials(id: string, credentials: import("@/lib/types").BasicCredentials | null): Promise<void> {
    await this.invoke("engine_set_credentials", { id, credentials });
  }

  async enqueueBatch(items: import("@/lib/types").BatchItem[], startPaused: boolean): Promise<import("@/lib/types").BatchResult[]> {
    await this.settingsWrites;
    return this.invoke("engine_enqueue_batch", { items, startPaused });
  }
  async automation(): Promise<import("@/lib/types").Automation> { return this.invoke("engine_automation"); }
  async captureStatus(): Promise<boolean> { return this.invoke("capture_status"); }
  async enableCapture(enabled: boolean): Promise<void> { return this.invoke("capture_enable", { enabled }); }
  async browserSetupStatus(): Promise<import("@/lib/browser-setup").BrowserSetupStatus> { return this.invoke("browser_setup_status"); }
  async registerBrowserHelper(browser: import("@/lib/browser-setup").BrowserTarget, extensionId: string, confirmed: boolean): Promise<void> { await this.invoke("browser_setup_register", { browser, extensionId, confirmed }); }
  async unregisterBrowserHelper(browser: import("@/lib/browser-setup").BrowserTarget, confirmed: boolean): Promise<void> { await this.invoke("browser_setup_unregister", { browser, confirmed }); }
  async openExtensionStore(browser: import("@/lib/browser-setup").BrowserTarget): Promise<void> { await this.invoke("browser_setup_open_store", { browser }); }
  async openExtensionFiles(browser: import("@/lib/browser-setup").BrowserTarget): Promise<void> { await this.invoke("browser_setup_open_files", { browser }); }
  async powerStatus(): Promise<import("@/lib/types").PowerPlan | null> { return this.invoke("power_status"); }
  async armPower(queueId: string, action: import("@/lib/types").CompletionAction, consent: boolean): Promise<void> { return this.invoke("power_arm", { queueId, action, consent }); }
  async cancelPower(): Promise<void> { return this.invoke("power_cancel"); }
  async acknowledgePower(): Promise<void> { return this.invoke("power_acknowledge"); }
  async autostartStatus(): Promise<boolean> { return this.invoke("desktop_autostart_status"); }
  async setAutostart(enabled: boolean): Promise<void> { return this.invoke("desktop_autostart", { enabled }); }
  async configureAutomation(command: import("@/lib/types").AutomationCommand): Promise<import("@/lib/types").Automation> {
    return this.invoke("engine_configure_automation", { command });
  }

  async updateProperties(id: string, properties: DownloadProperties): Promise<void> {
    await this.invoke("engine_update_properties", { id, properties });
  }

  async testSound(path: string): Promise<void> { await this.invoke("test_sound", { path }); }
  async dragCompleted(id: string): Promise<void> { await this.invoke("drag_completed", { id }); }

  async updateDestination(id: string, destination: string): Promise<void> {
    await this.invoke("engine_update_destination", { id, destination });
  }

  async probe(url: string): Promise<UrlProbe> {
    return this.invoke<UrlProbe>("engine_probe", { url });
  }

  async inspectMedia(url: string, allowedOrigins: string[], requestId: string, format: "hls" | "dash" = "hls"): Promise<import("@/lib/retrieval").MediaInspection> {
    return this.invoke("engine_inspect_media", { url, allowedOrigins, requestId, format });
  }
  async cancelMediaInspection(requestId: string): Promise<void> { await this.invoke("engine_cancel_inspection", { requestId }); }
  async mediaOffers(): Promise<import("@/lib/retrieval").MediaOffer[]> { return this.invoke("media_offers"); }
  async dismissMediaOffer(id: string): Promise<void> { await this.invoke("media_dismiss_offer", { id }); }
  async createMediaJob(request: import("@/lib/retrieval").MediaJobRequest): Promise<string> { return this.invoke("engine_create_media_job", { request }); }
  async mediaJobs(): Promise<import("@/lib/retrieval").MediaJob[]> { return this.invoke("engine_media_jobs"); }
  async removeCompletedMediaJob(id: string, confirmed: boolean): Promise<void> { await this.invoke("engine_remove_completed_media_job", { id, confirmed }); }
  async controlMediaJob(id: string, action: "pause" | "resume" | "cancel"): Promise<void> { await this.invoke("engine_control_media_job", { id, action }); }
  async createSiteProject(request: import("@/lib/site-retrieval").SiteProjectRequest): Promise<string> { return this.invoke("engine_create_site_project", { request }); }
  async siteProjects(): Promise<import("@/lib/site-retrieval").SiteProject[]> { return this.invoke("engine_site_projects"); }
  async controlSiteProject(id: string, action: import("@/lib/site-retrieval").SiteAction): Promise<void> { await this.invoke("engine_control_site_project", { id, action }); }
  async queueSiteResources(id: string, resources: string[]): Promise<string[]> { return this.invoke("engine_queue_site_resources", { id, resources }); }
  async configureSitePolicy(id: string, policy: import("@/lib/site-retrieval").SitePolicy, confirmed: boolean): Promise<void> { await this.invoke("engine_configure_site_policy", { id, policy, confirmed }); }
  async updateSiteFilters(id: string, extensions: string[], excludedPaths: string[], confirmed: boolean): Promise<void> { await this.invoke("engine_update_site_filters", { id, extensions, excludedPaths, confirmed }); }

  async pause(id: string): Promise<void> {
    await this.invoke("engine_pause", { id });
  }

  async resume(id: string): Promise<void> {
    await this.invoke("engine_resume", { id });
  }

  async retry(id: string): Promise<void> {
    await this.invoke("engine_retry", { id });
  }

  async updateUrl(id: string, url: string): Promise<void> {
    await this.invoke("engine_update_url", { id, url });
  }

  async checkForUpdates(): Promise<void> {
    await this.invoke("updates_check");
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
