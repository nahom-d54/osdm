import type { EngineBridge, EngineErrorListener, SnapshotListener } from "@/lib/bridge/types";
import type { Download, DownloadRequest, EngineConfig, Segment, UrlProbe } from "@/lib/types";
import { suggestFilename } from "@/lib/format";
import { joinPath } from "@/lib/format";
import { useSettings } from "@/store/settings";

export class BridgeError extends Error {}

function validateCredentials(source: string, credentials?: import("@/lib/types").BasicCredentials): void {
  if (!credentials) return;
  const url = new URL(source);
  const encoder = new TextEncoder();
  if (encoder.encode(credentials.username).length > 1024 || encoder.encode(credentials.password).length > 4096 || /[:\r\n]/.test(credentials.username) || /[\r\n]/.test(credentials.password)) {
    throw new BridgeError("Invalid Basic authentication credentials.");
  }
  if (url.protocol !== "https:" && !["127.0.0.1", "[::1]", "localhost"].includes(url.hostname)) throw new BridgeError("Credentials require HTTPS (except loopback testing).");
}

type Phase = "probe" | "run" | "publish";

interface Entry {
  download: Download;
  phase: Phase;
  ticksInPhase: number;
  /** Simulated per-segment rates in bytes per simulator tick. */
  rates: number[];
  stopped: boolean;
}

const TICK_MS = 250;

function totalSizeFor(url: string): number {
  // Deterministic pseudo-size from the URL so repeat demos behave consistently.
  let hash = 7;
  for (let i = 0; i < url.length; i++) hash = (hash * 31 + url.charCodeAt(i)) >>> 0;
  const mib = 24 + (hash % 300);
  return mib * 1024 * 1024;
}

/** 4 fixed disjoint ranges, the engine's default segmentation. */
function layoutSegments(total: number, parts: number): Segment[] {
  const size = total / parts;
  return Array.from({ length: parts }, (_, i) => ({
    start: Math.floor(size * i),
    end: i === parts - 1 ? total : Math.floor(size * (i + 1)),
    committed: 0,
    downloaded: 0,
  }));
}

function temporaryPathFor(destination: string): string {
  const dot = destination.lastIndexOf(".");
  return dot > 0 ? `${destination.slice(0, dot)}${".osdm-part."}${destination.slice(dot)}` : `${destination}.osdm-part`;
}

const CONTENT_TYPES: Record<string, string> = {
  iso: "application/x-iso9660-image",
  zip: "application/zip",
  exe: "application/vnd.microsoft.portable-executable",
  msi: "application/x-msi",
  pdf: "application/pdf",
  gguf: "application/octet-stream",
  mp4: "video/mp4",
  mkv: "video/x-matroska",
  zst: "application/zstd",
};

function contentTypeFor(value: string): string {
  const path = value.split("?")[0].split("#")[0];
  const extension = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
  return CONTENT_TYPES[extension] ?? "application/octet-stream";
}

/**
 * Simulates the engine contract for browser development: probe → segmented
 * transfer with checkpoints → hard-link publication. Failure injection is
 * triggered by URL fragments (`#fail-network`, `#slow`); `#server-name` gives
 * the mock probe a Content-Disposition name, and `#probe-fail` rejects the probe.
 */
export class MockBridge implements EngineBridge {
  readonly kind = "mock" as const;
  private entries: Entry[] = [];
  private automationState: import("@/lib/types").Automation = { queues: [], schedules: [], membership: {} };
  private config: EngineConfig = {
    max_active_downloads: 3,
    connections_per_download: 4,
    max_connections: 12,
    max_connections_per_host: 8,
    split_threshold: 16 * 1024 * 1024,
    bytes_per_second: 0,
    max_retries: 3,
  };
  private snapListeners = new Set<SnapshotListener>();
  private errListeners = new Set<EngineErrorListener>();
  private dirty = false;
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor() {
    // `?demo` fills the web mock with a representative library for design work.
    // The desktop shell never uses this bridge.
    if (new URLSearchParams(window.location.search).has("demo")) {
      this.seedDemo();
    }
  }

  async open(config: EngineConfig): Promise<void> {
    this.config = { ...config };
    this.startTicker();
  }

  async shutdown(): Promise<void> {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }

  private startTicker(): void {
    if (this.timer) return;
    this.timer = setInterval(() => this.tick(), TICK_MS);
  }

  private seedDemo(): void {
    const specs: DemoSpec[] = [
      {
        url: "https://releases.ubuntu.com/24.04.2/ubuntu-24.04.2-desktop-amd64.iso",
        destination: "C:\\Users\\you\\Downloads\\ubuntu-24.04.2-desktop-amd64.iso",
        state: "Downloading",
        totalMib: 5530,
        parts: 4,
        progress: 0.61,
        rateMib: 11.2,
      },
      {
        url: "https://get.enterprisedb.com/postgresql/postgresql-17.2-1-windows-x64.exe",
        destination: "C:\\Users\\you\\Downloads\\postgresql-17.2-1-windows-x64.exe",
        state: "Downloading",
        totalMib: 326,
        parts: 2,
        progress: 0.34,
        rateMib: 6.4,
      },
      {
        url: "https://huggingface.co/meta-llama/Llama-3.3-70B-Instruct/resolve/main/llama-3.3-70b-instruct-Q4_K_M.gguf",
        destination: "C:\\Users\\you\\Downloads\\llama-3.3-70b-instruct-Q4_K_M.gguf",
        state: "Downloading",
        totalMib: 40550,
        parts: 4,
        progress: 0.18,
        rateMib: 4.6,
      },
      {
        url: "https://files.archive.org/field-recordings-2f-2026.zip",
        destination: "C:\\Users\\you\\Downloads\\field-recordings-2f-2026.zip",
        state: "Paused",
        totalMib: 1946,
        parts: 4,
        progress: 0.47,
      },
      {
        url: "https://dev.mysql.com/get/Downloads/MySQLGUITools/mysql-workbench-community-8.0.41-winx64.msi",
        destination: "C:\\Users\\you\\Downloads\\mysql-workbench-community-8.0.41-winx64.msi",
        state: "Queued",
        totalMib: 72,
        parts: 1,
        progress: 0,
      },
      {
        url: "https://updates.signal.org/desktop/signal-desktop-win-7.42.0.exe",
        destination: "C:\\Users\\you\\Downloads\\signal-desktop-win-7.42.0.exe",
        state: "Failed",
        totalMib: 152,
        parts: 4,
        progress: 0.37,
        error: "network request failed",
      },
      {
        url: "https://teenage.engineering/guides/op-1-field-guide.pdf",
        destination: "C:\\Users\\you\\Downloads\\op-1-field-guide.pdf",
        state: "Completed",
        totalMib: 44,
        parts: 1,
        progress: 1,
      },
      {
        url: "https://static.rust-lang.org/dist/rust-1.89.0-x86_64-pc-windows-msvc.msi",
        destination: "C:\\Users\\you\\Downloads\\rust-1.89.0-x86_64-pc-windows-msvc.msi",
        state: "Completed",
        totalMib: 218,
        parts: 2,
        progress: 1,
      },
    ];
    this.entries = specs.map(seededEntry);
  }

  private markDirty(): void {
    this.dirty = true;
  }

  private pushSnapshot(): void {
    const ordered = this.entries.map((e) => e.download);
    for (const listener of this.snapListeners) listener(ordered);
  }

  // --- lifecycle commands ---

  async enqueue(request: DownloadRequest, startPaused = false, properties?: import("@/lib/types").DownloadProperties, credentials?: import("@/lib/types").BasicCredentials): Promise<string> {
    validateCredentials(request.url, credentials);
    if (request.expected_sha256 && !/^[a-f0-9]{64}$/i.test(request.expected_sha256.trim())) {
      throw new BridgeError("SHA-256 must contain exactly 64 hexadecimal characters.");
    }
    if (this.entries.some((e) => e.download.destination === request.destination)) {
      throw new BridgeError("A file already exists at this destination. The engine never overwrites.");
    }
    const id = crypto.randomUUID();
    const temporaryDirectory = useSettings.getState().settings.temporary_directory;
    const entry: Entry = {
      download: {
        id,
        url: request.url,
        destination: request.destination,
        properties: properties ?? { description: "", category_id: null },
        temporary_path: temporaryDirectory ? joinPath(temporaryDirectory, `.osdm-${id}.part`) : temporaryPathFor(request.destination),
        temporary_directory: temporaryDirectory || null,
        state: startPaused ? "Paused" : "Queued",
        total_bytes: null,
        segments: [{ start: 0, end: null, committed: 0, downloaded: 0 }],
        error: null,
        bytes_per_second: 0,
        eta_seconds: null,
        expected_sha256: request.expected_sha256?.trim().toLowerCase() || null,
        integrity: "Unverified",
        validator: null,
        ranged: true,
      },
      phase: "probe",
      ticksInPhase: 0,
      rates: [],
      stopped: false,
    };
    this.entries.push(entry);
    this.markDirty();
    return id;
  }

  async updateProperties(id: string, properties: import("@/lib/types").DownloadProperties): Promise<void> {
    if (properties.description.length > 4096 || (properties.category_id?.length ?? 0) > 128) throw new BridgeError("Invalid properties.");
    this.find(id).download.properties = { ...properties };
    this.markDirty();
  }

  async automation(): Promise<import("@/lib/types").Automation> { return structuredClone(this.automationState); }
  async captureStatus(): Promise<boolean> { return false; }
  async enableCapture(_enabled: boolean): Promise<void> { throw new BridgeError("Browser integration requires the native desktop app and registered host."); }
  async powerStatus(): Promise<import("@/lib/types").PowerPlan | null> { return null; }
  async armPower(_queueId: string, _action: import("@/lib/types").CompletionAction, _consent: boolean): Promise<void> { throw new BridgeError("Completion actions require the native Windows app."); }
  async cancelPower(): Promise<void> {}
  async acknowledgePower(): Promise<void> {}
  async autostartStatus(): Promise<boolean> { return false; }
  async setAutostart(_enabled: boolean): Promise<void> { throw new BridgeError("Start with Windows requires the native Windows app."); }
  async configureAutomation(command: import("@/lib/types").AutomationCommand): Promise<import("@/lib/types").Automation> {
    const next = structuredClone(this.automationState);
    if (command.action === "save_queue") {
      if (!command.name.trim() || new TextEncoder().encode(command.name).length > 128) throw new BridgeError("Invalid queue name.");
      const old = next.queues.find((q) => q.id === command.id);
      if (old) old.name = command.name.trim();
      else next.queues.push({ id: command.id, name: command.name.trim(), running: false, stop_at: null });
    } else if (command.action === "delete_queue") {
      if (next.queues.some((q) => q.id === command.id && q.running)) throw new BridgeError("Stop queue before deletion.");
      next.queues = next.queues.filter((q) => q.id !== command.id);
      next.schedules = next.schedules.filter((s) => s.queue_id !== command.id);
      next.membership = Object.fromEntries(Object.entries(next.membership).filter(([, q]) => q !== command.id));
    } else if (command.action === "assign") {
      if (command.queue_id && !next.queues.some((q) => q.id === command.queue_id)) throw new BridgeError("Unknown queue.");
      for (const id of command.ids) {
        if (!["Paused", "Completed", "Failed", "Canceled"].includes(this.find(id).download.state)) throw new BridgeError("Pause before queue assignment.");
        if (command.queue_id) next.membership[id] = command.queue_id;
        else delete next.membership[id];
      }
    } else if (command.action === "save_schedule") {
      const s = command.schedule;
      if (!next.queues.some((q) => q.id === s.queue_id) || !Number.isSafeInteger(s.next_run) || s.next_run < 0 || (s.interval_seconds != null && (s.interval_seconds < 60 || s.interval_seconds > 31536000))) throw new BridgeError("Invalid schedule.");
      next.schedules = [...next.schedules.filter((s) => s.id !== command.schedule.id), command.schedule];
    } else if (command.action === "delete_schedule") next.schedules = next.schedules.filter((s) => s.id !== command.id);
    else if (command.action === "set_synchronization") {
      const queue = next.queues.find((q) => q.id === command.id);
      if (!queue || queue.running) throw new BridgeError("Stop a known queue before changing synchronization policy.");
      // Browser mock cannot establish real remote identity; do not fabricate a
      // synchronization result from its simulated sizes/progress.
      if (command.enabled) throw new BridgeError("Remote revision checks require the native engine.");
      queue.synchronize = false;
    }
    else {
      const queue = next.queues.find((q) => q.id === command.id);
      if (!queue) throw new BridgeError("Unknown queue.");
      queue.running = command.action === "start";
      for (const [id, q] of Object.entries(next.membership)) if (q === queue.id) {
        const d = this.find(id).download;
        if (command.action === "start" && d.state === "Paused") await this.resume(id);
        if (command.action === "stop" && ["Queued", "Downloading", "Pausing"].includes(d.state)) await this.pause(id);
      }
    }
    this.automationState = next;
    return this.automation();
  }

  async enqueueBatch(items: import("@/lib/types").BatchItem[], startPaused: boolean): Promise<import("@/lib/types").BatchResult[]> {
    if (!items.length || items.length > 1000) throw new BridgeError("Batch must contain 1 to 1000 requests.");
    const results: import("@/lib/types").BatchResult[] = [];
    for (const [index, item] of items.entries()) {
      let id: string | null = null;
      let duplicate = false;
      try {
        const url = new URL(item.request.url); url.hash = "";
        if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) throw new BridgeError("Invalid URL.");
        if (item.queue_id && !this.automationState.queues.some((q) => q.id === item.queue_id)) throw new BridgeError("Unknown queue.");
        const old = this.entries.find((e) => e.download.url === url.href && e.download.destination === item.request.destination);
        duplicate = !!old;
        id = old?.download.id ?? await this.enqueue({ ...item.request, url: url.href }, true);
        if (item.queue_id) await this.configureAutomation({ action: "assign", ids: [id], queue_id: item.queue_id });
        if (!startPaused) await this.resume(id);
        results.push({ index, id, duplicate, error: null });
      } catch { results.push({ index, id, duplicate, error: "admission_failed" }); }
    }
    return results;
  }

  async testSound(_path: string): Promise<void> { throw new BridgeError("Sound tests require the native Windows app."); }
  async dragCompleted(_id: string): Promise<void> { throw new BridgeError("File drag-out requires the native app."); }
  async setCredentials(id: string, _credentials: import("@/lib/types").BasicCredentials | null): Promise<void> {
    const download = this.find(id).download;
    if (!["Paused", "Failed"].includes(download.state)) throw new BridgeError("Pause before changing credentials.");
    validateCredentials(download.url, _credentials ?? undefined);
  }

  async updateDestination(id: string, destination: string): Promise<void> {
    const download = this.find(id).download;
    if (!["Paused", "Failed", "Canceled"].includes(download.state)) throw new BridgeError("Pause the download before editing its destination.");
    if (this.entries.some((entry) => entry.download.id !== id && entry.download.destination === destination)) throw new BridgeError("Destination is reserved.");
    const parent = (path: string) => path.slice(0, Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")));
    if (parent(destination) !== parent(download.destination) && download.segments.some((segment) => segment.downloaded > 0)) throw new BridgeError("Partial data exists; keep its folder and edit only the filename.");
    if (parent(destination) !== parent(download.destination) && !download.temporary_directory) download.temporary_path = temporaryPathFor(destination);
    download.destination = destination;
    this.markDirty();
  }

  async probe(url: string): Promise<UrlProbe> {
    await new Promise((resolve) => setTimeout(resolve, 450));
    let parsed: URL;
    try {
      parsed = new URL(url);
    } catch {
      throw new BridgeError("Enter a valid HTTP(S) URL.");
    }
    if (url.includes("#probe-fail")) {
      throw new BridgeError("network request failed");
    }
    const serverHint = parsed.hash === "#server-name";
    const requestedName = parsed.searchParams.get("filename");
    parsed.hash = "";
    const finalUrl = parsed.toString();
    const filename = requestedName ?? (serverHint ? "server-named-download.zip" : null);
    return {
      final_url: finalUrl,
      filename,
      content_type: contentTypeFor(filename ?? finalUrl),
      total_bytes: totalSizeFor(finalUrl),
      ranged: true,
    };
  }

  private find(id: string): Entry {
    const entry = this.entries.find((e) => e.download.id === id);
    if (!entry) throw new BridgeError("Download not found.");
    return entry;
  }

  async pause(id: string): Promise<void> {
    const entry = this.find(id);
    const { state } = entry.download;
    if (state === "Queued") {
      entry.download.state = "Paused";
    } else if (state === "Downloading") {
      entry.download.state = "Pausing";
      entry.ticksInPhase = 0;
    }
    // Resume is a no-op for queued/active/completed downloads.
    this.markDirty();
  }

  async resume(id: string): Promise<void> {
    const entry = this.find(id);
    if (entry.download.state === "Paused") {
      // Recovery rule: live counts reset to committed on restart.
      for (const seg of entry.download.segments) seg.downloaded = seg.committed;
      entry.download.state = "Queued";
      if (entry.phase === "publish") entry.phase = "run";
      this.markDirty();
    }
  }

  async retry(id: string): Promise<void> {
    const entry = this.find(id);
    const { state } = entry.download;
    if (state !== "Failed" && state !== "Canceled") return;
    entry.download.error = null;
    entry.download.state = "Queued";
    if (entry.phase !== "run" && entry.phase !== "publish") entry.phase = "run";
    // Revalidate before resuming; keep committed progress per the engine contract.
    for (const seg of entry.download.segments) seg.downloaded = seg.committed;
    this.markDirty();
  }

  async cancel(id: string): Promise<void> {
    const entry = this.find(id);
    const { state } = entry.download;
    if (["Queued", "Downloading", "Pausing", "Paused"].includes(state)) {
      entry.download.state = "Canceling";
      entry.ticksInPhase = 0;
    }
    this.markDirty();
  }

  async checkForUpdates(): Promise<void> {
    console.info("[mock] update check requested");
  }

  async updateUrl(id: string, raw: string): Promise<void> {
    const url = new URL(raw.trim());
    if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) {
      throw new BridgeError("Use an HTTP(S) URL without embedded credentials.");
    }
    const entry = this.find(id);
    if (!["Paused", "Failed"].includes(entry.download.state)) {
      throw new BridgeError("Pause the download before changing its URL.");
    }
    entry.download.url = url.toString();
    entry.download.state = "Paused";
    entry.download.error = null;
    entry.download.ready_to_publish = false;
    entry.download.sha256 = null;
    entry.download.integrity = "Unverified";
    entry.phase = "run";
    this.markDirty();
  }

  async reorder(ids: string[]): Promise<void> {
    const byId = new Map(this.entries.map((e) => [e.download.id, e]));
    if (ids.length !== this.entries.length || new Set(ids).size !== ids.length || ids.some((id) => !byId.has(id))) {
      throw new BridgeError("Reorder requires every download id exactly once.");
    }
    this.entries = ids.map((id) => byId.get(id)!);
    this.markDirty();
  }

  async setSpeedLimit(bytesPerSecond: number): Promise<void> {
    this.config.bytes_per_second = bytesPerSecond;
    this.markDirty();
  }

  async snapshot(): Promise<Download[]> {
    return this.entries.map((e) => e.download);
  }

  async removeDownload(id: string, removePartialFile: boolean): Promise<void> {
    const entry = this.find(id);
    if (!["Completed", "Failed", "Canceled"].includes(entry.download.state)) {
      throw new BridgeError("Pause or cancel the download before deleting it.");
    }
    if (removePartialFile) {
      // The engine command (pending) deletes the temporary file it owns.
    }
    this.entries = this.entries.filter((e) => e.download.id !== id);
    this.markDirty();
  }

  async cleanupPartialData(): Promise<number> {
    const removable = this.entries.filter((e) =>
      ["Failed", "Canceled"].includes(e.download.state) && e.download.segments.some((s) => s.committed > 0),
    );
    this.markDirty();
    return removable.length;
  }

  subscribe(listener: SnapshotListener): () => void {
    this.snapListeners.add(listener);
    listener(this.entries.map((e) => e.download));
    return () => this.snapListeners.delete(listener);
  }

  onError(listener: EngineErrorListener): () => void {
    this.errListeners.add(listener);
    return () => this.errListeners.delete(listener);
  }

  // --- desktop workflows (mocked) ---

  async pickDirectory(_title: string, defaultPath?: string): Promise<string | null> {
    await new Promise((r) => setTimeout(r, 250));
    return defaultPath ?? "C:\\Users\\you\\Downloads";
  }

  async defaultDownloadDirectory(): Promise<string> {
    return "C:\\Users\\you\\Downloads";
  }

  async suggestFilename(url: string): Promise<string> {
    return suggestFilename(url);
  }

  async pathExists(path: string): Promise<boolean> {
    return this.entries.some((e) => e.download.destination === path) || path.toLowerCase().endsWith("taken.bin");
  }

  async openPath(path: string): Promise<void> {
    // In the Tauri app this shells out via the opener plugin.
    console.info(`[mock] open ${path}`);
  }

  async revealPath(path: string): Promise<void> {
    console.info(`[mock] reveal ${path}`);
  }

  // --- simulator ---

  private tick(): void {
    const now = Math.floor(Date.now() / 1000);
    for (const schedule of this.automationState.schedules) {
      if (!schedule.enabled || schedule.next_run > now) continue;
      const late = now - schedule.next_run;
      const queue = this.automationState.queues.find((q) => q.id === schedule.queue_id);
      if (queue && !queue.running && late <= 30) {
        queue.running = true;
        queue.stop_at = schedule.stop_after_seconds == null ? null : now + schedule.stop_after_seconds;
        for (const entry of this.entries) if (this.automationState.membership[entry.download.id] === queue.id && entry.download.state === "Paused") { entry.download.state = "Queued"; this.markDirty(); }
      }
      if (schedule.interval_seconds) schedule.next_run += (Math.floor(late / schedule.interval_seconds) + 1) * schedule.interval_seconds;
      else schedule.enabled = false;
    }
    for (const queue of this.automationState.queues) {
      if (queue.stop_at != null && queue.stop_at <= now) {
        queue.running = false; queue.stop_at = null;
        for (const entry of this.entries) if (this.automationState.membership[entry.download.id] === queue.id && ["Downloading", "Queued", "Pausing"].includes(entry.download.state)) { entry.download.state = "Paused"; entry.download.bytes_per_second = 0; this.markDirty(); }
      }
      if (queue.running && !this.entries.some((entry) => this.automationState.membership[entry.download.id] === queue.id && ["Queued", "Downloading", "Pausing"].includes(entry.download.state))) { queue.running = false; queue.stop_at = null; }
    }
    const activeCount = this.entries.filter((e) => e.download.state === "Downloading").length;
    let budgetUsed = 0;
    const capBytesPerTick = this.config.bytes_per_second > 0 ? this.config.bytes_per_second * (TICK_MS / 1000) : Infinity;

    // Admit queued downloads while slots are free.
    let slots = Math.max(0, this.config.max_active_downloads - activeCount);
    for (const entry of this.entries) {
      if (slots <= 0) break;
      if (entry.download.state === "Queued") {
        entry.download.state = "Downloading";
        entry.phase = "probe";
        entry.ticksInPhase = 0;
        slots--;
        this.markDirty();
      }
    }

    for (const entry of this.entries) {
      const d = entry.download;
      switch (d.state) {
        case "Pausing":
          entry.ticksInPhase++;
          if (entry.ticksInPhase >= 2) {
            d.state = "Paused";
            d.bytes_per_second = 0;
            d.eta_seconds = null;
            entry.ticksInPhase = 0;
            this.markDirty();
          }
          break;
        case "Canceling":
          entry.ticksInPhase++;
          if (entry.ticksInPhase >= 2) {
            d.state = "Canceled";
            d.bytes_per_second = 0;
            d.eta_seconds = null;
            entry.ticksInPhase = 0;
            this.markDirty();
          }
          break;
        case "Downloading":
          this.advanceDownload(entry);
          break;
        default:
          break;
      }
    }

    // Enforce the aggregate cap roughly (simulation, not accounting).
    void budgetUsed;
    void capBytesPerTick;

    if (this.dirty) {
      this.dirty = false;
      this.pushSnapshot();
    }
  }

  private advanceDownload(entry: Entry): void {
    const d = entry.download;

    if (entry.phase === "probe") {
      entry.ticksInPhase++;
      if (entry.ticksInPhase >= 2) {
        // Probe result: range support depends on the engine.
        const total = totalSizeFor(d.url);
        const parts = total > this.config.split_threshold ? Math.min(4, this.config.connections_per_download) : 1;
        d.total_bytes = total;
        d.segments = layoutSegments(total, parts);
        d.validator = `W/"osdm-${Math.abs(hashString(d.url)).toString(16)}"`;
        entry.rates = d.segments.map(
          (s) => ((s.end! - s.start) / parts === 0 ? 0 : (0.9 + pseudoRandom(s.start, d.url) * 1.4) * 1024 * 256), // ~0.9–2.3 MiB per second
        );
        entry.phase = "run";
        entry.ticksInPhase = 0;
        this.markDirty();
      }
      return;
    }

    if (entry.phase === "run") {
      const seconds = TICK_MS / 1000;
      let liveDelta = 0;
      d.segments.forEach((seg, i) => {
        if (seg.downloaded >= seg.end! - seg.start) return;
        let rate = entry.rates[i];
        if (d.url.includes("#slow")) rate *= 0.08;
        const chunk = Math.min(rate * seconds, seg.end! - seg.start - seg.downloaded);
        seg.downloaded += chunk;
        liveDelta += chunk;
        // Checkpoint ~1 MiB per worker: committed lags live until the boundary.
        const checkpoint = 1024 * 1024;
        const target = Math.floor(seg.downloaded / checkpoint) * checkpoint;
        if (target > seg.committed) seg.committed = Math.min(target, seg.downloaded);
      });

      // Injected failure.
      const done = d.segments.every((s) => s.downloaded >= s.end! - s.start);
      if (d.url.includes("#fail-network") && !done && d.segments.some((s) => s.downloaded > 40 * 1024 * 1024)) {
        d.state = "Failed";
        d.error = "network request failed";
        d.bytes_per_second = 0;
        d.eta_seconds = null;
        this.markDirty();
        return;
      }

      if (done) {
        for (const seg of d.segments) seg.committed = seg.downloaded;
        entry.phase = "publish";
        entry.ticksInPhase = 0;
        d.ready_to_publish = true;
      } else {
        d.bytes_per_second = liveDelta * 4;
        d.eta_seconds = estimateEta(d);
        this.markDirty();
      }
      return;
    }

    if (entry.phase === "publish") {
      entry.ticksInPhase++;
      if (entry.ticksInPhase >= 1) {
        d.state = "Completed";
        d.bytes_per_second = 0;
        d.eta_seconds = null;
        this.markDirty();
      }
    }
  }
}

interface DemoSpec {
  url: string;
  destination: string;
  state: Download["state"];
  totalMib: number;
  parts: number;
  progress: number;
  rateMib?: number;
  error?: string;
}

function seededEntry(spec: DemoSpec): Entry {
  const MIB = 1024 * 1024;
  const total = spec.totalMib * MIB;
  const parts = Math.max(1, spec.parts);
  const segments = layoutSegments(total, parts);
  const checkpoint = MIB;

  for (const segment of segments) {
    const size = (segment.end ?? 0) - segment.start;
    const committed = Math.min(Math.floor((size * spec.progress) / checkpoint) * checkpoint, size);
    segment.committed = spec.state === "Completed" ? size : committed;
    segment.downloaded =
      spec.state === "Downloading"
        ? Math.min(size, segment.committed + MIB * 0.35)
        : segment.committed;
  }

  const rate = spec.rateMib ? spec.rateMib * MIB : 0;
  const received = segments.reduce((sum, segment) => sum + segment.downloaded, 0);
  const remaining = Math.max(0, total - received);

  return {
    download: {
      id: crypto.randomUUID(),
      url: spec.url,
      destination: spec.destination,
      temporary_path: temporaryPathFor(spec.destination),
      state: spec.state,
      total_bytes: total,
      segments,
      error: spec.error ?? null,
      bytes_per_second: spec.state === "Downloading" ? rate : 0,
      eta_seconds: spec.state === "Downloading" && rate > 0 ? Math.ceil(remaining / rate) : null,
      validator: `W/"osdm-demo-${spec.totalMib}-${parts}"`,
      ranged: true,
    },
    phase: spec.state === "Completed" ? "publish" : "run",
    ticksInPhase: 0,
    rates: spec.state === "Downloading" ? segments.map(() => rate / parts) : [],
    stopped: false,
  };
}

function estimateEta(d: Download): number | null {
  if (d.total_bytes == null || d.bytes_per_second <= 0) return null;
  const remaining = d.total_bytes - d.segments.reduce((a, s) => a + s.downloaded, 0);
  return remaining <= 0 ? 0 : Math.ceil(remaining / d.bytes_per_second);
}

function hashString(value: string): number {
  let hash = 7;
  for (let i = 0; i < value.length; i++) hash = (hash * 31 + value.charCodeAt(i)) | 0;
  return hash;
}

function pseudoRandom(seed: number, salt: string): number {
  let hash = seed >>> 0;
  for (let i = 0; i < salt.length; i++) hash = (hash * 1103515245 + salt.charCodeAt(i)) >>> 0;
  return ((hash >>> 8) & 0xff) / 255;
}
