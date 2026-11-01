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

export interface SiteResource {
  id: string;
  url: string;
  relative_path: string;
  size: number | null;
  download_id: string | null;
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
}

export type SiteAction = "pause" | "resume" | "cancel" | "delete";

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
