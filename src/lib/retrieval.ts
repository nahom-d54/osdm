/** Source URLs are secrets. Do not log these payloads or persist browser offers. */
export interface MediaOffer {
  id: string;
  url: string;
  title: string;
  subtitle_url: string | null;
  kind: "hls" | "dash" | "direct" | "unknown";
}

export interface MediaChoice {
  url: string;
  /** Decimal u64 text preserves manifest metadata beyond JavaScript's safe integers. */
  bandwidth: string;
  resolution: [number, number] | null;
  codecs: string | null;
  track_key?: string | null;
  track_kind?: "video" | "audio" | "subtitle" | null;
  language?: string | null;
  segment_count?: number;
  hls_audio_group?: string | null;
  hls_renditions?: Array<{ key: string; kind: "audio" | "subtitle"; name: string; language: string | null; in_band: boolean; default: boolean; forced: boolean }>;
}

export interface MediaInspection {
  kind: "master" | "vod" | "dash";
  choices: MediaChoice[];
  segment_count: number;
  resource_origins: string[];
}

export interface MediaJobRequest {
  url: string;
  destination: string;
  allowed_origins: string[];
  ffmpeg_path: string;
  description: string | null;
  subtitle_url?: string | null;
  category_id?: string | null;
  dash_tracks?: string[];
  direct_media?: boolean;
  hls_variant_key?: string | null;
  hls_audio_key?: string | null;
  hls_subtitle_key?: string | null;
}

export interface MediaJob {
  id: string;
  destination: string;
  description: string | null;
  category_id?: string | null;
  state: "Paused" | "Downloading" | "Merging" | "Publishing" | "Completed" | "Failed" | "Canceled";
  children: string[];
  completed_segments: number;
  total_segments: number;
  sha256: string | null;
  error: string | null;
  removal_pending?: boolean;
}

export function authorizedOrigins(text: string): string[] {
  const values = text.split(/\s+/).filter(Boolean);
  if (values.length > 32) throw new Error("Approve at most 32 additional origins.");
  return [...new Set(values.map((value) => {
    let url: URL;
    try { url = new URL(value); } catch { throw new Error("Enter HTTP(S) origins, without paths or queries."); }
    if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || value !== url.origin) {
      throw new Error("Enter HTTP(S) origins, without paths, credentials, queries, or a trailing slash.");
    }
    return url.origin;
  }))];
}

export function mediaSource(value: string): string {
  if (value !== value.trim() || value.length > 8192 || /[\u0000-\u001f\u007f]/.test(value)) throw new Error("Enter an HTTP(S) media URL without credentials or a fragment.");
  let url: URL;
  try { url = new URL(value); } catch { throw new Error("Enter an HTTP(S) media URL."); }
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || url.hash) throw new Error("Enter an HTTP(S) media URL without credentials or a fragment.");
  return url.href;
}

export function qualityLabel(choice: MediaChoice): string {
  const resolution = choice.resolution ? `${choice.resolution[0]} × ${choice.resolution[1]}` : "Resolution not declared";
  return `${resolution} · ${BigInt(choice.bandwidth).toLocaleString()} bit/s declared`;
}
