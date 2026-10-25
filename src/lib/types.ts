// Mirror of `crates/download-engine/src/model.rs` serde output.
// Field names use snake_case because the Rust structs derive Serialize
// without rename attributes. Keep both sides in step.

/** UUID string as produced by the engine's DownloadId. */
export type DownloadId = string;

export type DownloadState =
  | "Queued"
  | "Downloading"
  | "Pausing"
  | "Paused"
  | "Canceling"
  | "Canceled"
  | "Failed"
  | "Completed";

export interface Segment {
  start: number;
  /** Exclusive end. Unknown only for a single streaming transfer. */
  end: number | null;
  /** Bytes known to be synced to disk. Survives restart. */
  committed: number;
  /** Live bytes; reset to committed after recovery. */
  downloaded: number;
}

export interface Download {
  id: DownloadId;
  url: string;
  destination: string;
  temporary_path: string;
  state: DownloadState;
  total_bytes: number | null;
  segments: Segment[];
  error: string | null;
  bytes_per_second: number;
  eta_seconds: number | null;
  /** Strong ETag or Last-Modified string, when the server supplied one. */
  validator: string | null;
  ranged: boolean;
  ready_to_publish?: boolean;
}

export type EngineConfig = {
  max_active_downloads: number;
  connections_per_download: number;
  max_connections: number;
  max_connections_per_host: number;
  split_threshold: number;
  /** Zero means unlimited. */
  bytes_per_second: number;
  max_retries: number;
};

export interface DownloadRequest {
  url: string;
  /** Absolute destination filename; the engine never overwrites. */
  destination: string;
}

/** Metadata returned by a pre-download probe; no queue entry is created. */
export interface UrlProbe {
  /** URL after redirects. */
  final_url: string;
  /** Content-Disposition filename, with path components removed. */
  filename: string | null;
  content_type: string | null;
  total_bytes: number | null;
  ranged: boolean;
}

export function downloadedBytes(download: Download): number {
  return download.segments.reduce((sum, s) => sum + s.downloaded, 0);
}

export function committedBytes(download: Download): number {
  return download.segments.reduce((sum, s) => sum + s.committed, 0);
}

export const ACTIVE_STATES: DownloadState[] = ["Queued", "Downloading", "Pausing", "Paused"];
/** Terminal in history: keep or delete; the engine never drops them silently. */
export const TERMINAL_STATES: DownloadState[] = ["Canceled", "Failed", "Completed"];
