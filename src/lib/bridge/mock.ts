import type { EngineBridge, EngineErrorListener, SnapshotListener } from "@/lib/bridge/types";
import type { Download, EngineConfig, Segment } from "@/lib/types";
import { suggestFilename } from "@/lib/format";

export class BridgeError extends Error {}

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

/**
 * Simulates the engine contract for browser development: probe → segmented
 * transfer with checkpoints → hard-link publication. Failure injection is
 * triggered by URL fragments (`#fail-network`, `#slow`).
 */
export class MockBridge implements EngineBridge {
  readonly kind = "mock" as const;
  private entries: Entry[] = [];
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

  private markDirty(): void {
    this.dirty = true;
  }

  private pushSnapshot(): void {
    const ordered = this.entries.map((e) => e.download);
    for (const listener of this.snapListeners) listener(ordered);
  }

  // --- lifecycle commands ---

  async enqueue(request: { url: string; destination: string }): Promise<string> {
    if (this.entries.some((e) => e.download.destination === request.destination)) {
      throw new BridgeError("A file already exists at this destination. The engine never overwrites.");
    }
    const id = crypto.randomUUID();
    const entry: Entry = {
      download: {
        id,
        url: request.url,
        destination: request.destination,
        temporary_path: temporaryPathFor(request.destination),
        state: "Queued",
        total_bytes: null,
        segments: [{ start: 0, end: null, committed: 0, downloaded: 0 }],
        error: null,
        bytes_per_second: 0,
        eta_seconds: null,
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
