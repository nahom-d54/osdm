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
  properties?: DownloadProperties;
  temporary_path: string;
  temporary_directory?: string | null;
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
  adaptive?: {
    active_connections: number;
    host_connections: number;
    host_limit: number;
    host_ceiling: number;
    chunk_bytes: number;
    reason: string;
  } | null;
  expected_sha256?: string | null;
  sha256?: string | null;
  integrity?: "Unverified" | "Verifying" | "Computed" | "Verified" | "Mismatch";
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
  expected_sha256?: string | null;
}

/** Durable non-secret organization metadata. */
export interface DownloadProperties {
  description: string;
  category_id: string | null;
}

/** Input only, session-only. Never store this in Zustand persistence. */
export interface BasicCredentials { username: string; password: string }

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
