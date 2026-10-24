const KB = 1024;

/** IEC bytes with common binary labels (KB/MB/GB), the download-manager convention. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < KB) return `${Math.round(bytes)} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = "B";
  for (const next of units) {
    if (value < KB) break;
    value /= KB;
    unit = next;
  }
  const digits = value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${unit}`;
}

export function formatSpeed(bytesPerSecond: number): string {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) return "—";
  return `${formatBytes(bytesPerSecond)}/s`;
}

export function formatEta(seconds: number | null): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return "—";
  const s = Math.floor(seconds);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}

export function formatPercent(downloaded: number, total: number | null): string {
  if (total == null || total <= 0) return "";
  return `${Math.min(100, Math.floor((downloaded / total) * 100))}%`;
}

/** Monotonic clock string for history entries, e.g. "14:03". */
export function formatClock(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** Filename suggestion derived from a URL, percent-decoded and sanitized. */
export function suggestFilename(url: string): string {
  let name: string | undefined;
  try {
    const parsed = new URL(url);
    const base = parsed.pathname.split("/").filter(Boolean).pop();
    if (base) name = base;
  } catch {
    // fall through; URL is validated elsewhere
  }
  if (!name) name = "download.bin";
  const decoded = safeDecode(name);
  const dot = decoded.lastIndexOf(".");
  const stem = dot > 0 ? decoded.slice(0, dot) : decoded;
  const ext = dot > 0 ? decoded.slice(dot + 1).toLowerCase() : "";
  const cleanStem = sanitize(stem) || "download";
  const cleanExt = ext && /^[a-z0-9]{1,12}$/.test(ext) ? `.${ext}` : "";
  return `${cleanStem}${cleanExt}`;
}

function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

/** Characters Windows rejects in filenames. */
function sanitize(value: string): string {
  const cleaned = value.replace(/[<>:"/\\|?*\u0000-\u001f]+/g, " ").replace(/\s+/g, " ").trim();
  // Windows reserves these stems.
  const reserved = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\.|$)/i;
  if (reserved.test(cleaned)) return `_${cleaned}`;
  return cleaned;
}
