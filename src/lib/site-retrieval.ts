export interface SiteProjectRequest {
  name: string;
  seed_url: string;
  path_prefix: string;
  destination: string;
  extensions: string[];
  excluded_paths: string[];
  max_pages: number;
  max_files: number;
  max_bytes: number;
  max_depth: number;
  confirmed: boolean;
}

/** Scope-free presets: never retain a source, destination, consent, or schedule. */
export const SITE_TEMPLATES = [
  { id: "documents", name: "Documents", extensions: "pdf txt epub doc docx odt", pages: "20", files: "100", mebibytes: "64", depth: "3" },
  { id: "images", name: "Images", extensions: "png jpg jpeg gif webp svg avif", pages: "20", files: "200", mebibytes: "128", depth: "2" },
  { id: "audio", name: "Audio", extensions: "mp3 m4a ogg opus flac wav", pages: "10", files: "50", mebibytes: "256", depth: "2" },
  { id: "website", name: "Website resources", extensions: "pdf html htm png jpg svg css", pages: "20", files: "100", mebibytes: "64", depth: "3" },
] as const;

export interface SiteResource {
  id: string;
  url: string;
  relative_path: string;
  size: number | null;
  download_id: string | null;
  description?: string;
}

export interface SiteProject {
  id: string;
  name: string;
  destination: string;
  state: "Paused" | "Exploring" | "Ready" | "Failed" | "Canceled";
  pages: number;
  bytes: number;
  resources: SiteResource[];
  error: string | null;
  policy?: SitePolicy;
  generation?: number;
  last_run?: number | null;
  schedule_note?: string | null;
  extensions?: string[];
  excluded_paths?: string[];
}

export interface SitePolicy {
  automatic_downloads: boolean;
  max_concurrent_downloads: number;
  cache_responses: boolean;
  next_run: number | null;
  interval_seconds: number | null;
  catch_up_on_startup: boolean;
}

export const DEFAULT_SITE_POLICY: SitePolicy = {
  automatic_downloads: false, max_concurrent_downloads: 1, cache_responses: false,
  next_run: null, interval_seconds: null, catch_up_on_startup: false,
};

export function utcSiteSchedule(value: string, minutes: string): Pick<SitePolicy, "next_run" | "interval_seconds"> {
  if (!value) return { next_run: null, interval_seconds: null };
  if (!/^\d{4}-\d\d-\d\dT\d\d:\d\d$/.test(value)) throw new Error("Enter a valid UTC date and time.");
  const next = Date.parse(`${value}:00Z`) / 1000;
  if (!Number.isSafeInteger(next) || next < 0 || next > 253402300799 || new Date(next * 1000).toISOString().slice(0, 16) !== value) throw new Error("Enter a valid UTC date and time.");
  if (minutes && (!/^\d+$/.test(minutes) || Number(minutes) < 1 || Number(minutes) > 525600)) throw new Error("Repeat minutes must be a whole number from 1 to 525,600.");
  return { next_run: next, interval_seconds: minutes ? Number(minutes) * 60 : null };
}

export type SiteAction = "pause" | "resume" | "cancel" | "delete" | "recheck";

export function scopeFor(seed: string): string {
  try {
    const url = new URL(seed);
    return url.pathname.endsWith("/") ? url.pathname : url.pathname.slice(0, url.pathname.lastIndexOf("/") + 1);
  } catch { return "/"; }
}

export function siteLimits(value: { pages: string; files: string; mebibytes: string; depth: string }) {
  const positive = (raw: string, max: number) => {
    if (!/^\d+$/.test(raw)) throw new Error("Budgets must be whole numbers.");
    const number = Number(raw);
    if (!Number.isSafeInteger(number) || number < 1 || number > max) throw new Error("Choose finite budgets within the displayed limits.");
    return number;
  };
  return { max_pages: positive(value.pages, 1000), max_files: positive(value.files, 10000), max_bytes: positive(value.mebibytes, 10240) * 1048576, max_depth: positive(value.depth, 16) };
}
