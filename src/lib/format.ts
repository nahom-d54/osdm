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

/** Last path segment, tolerant of Windows and POSIX separators. */
export function fileNameOf(path: string): string {
  const cleaned = path.replace(/\\/g, "/");
  return cleaned.slice(cleaned.lastIndexOf("/") + 1) || path;
}

/** File extension as a short uppercase badge, e.g. "ISO", "7Z", "TAR". */
export function extensionOf(path: string): string {
  const name = fileNameOf(path);
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return "FILE";
  const extension = name.slice(dot + 1).replace(/[^a-z0-9]/gi, "").slice(0, 4);
  return extension ? extension.toUpperCase() : "FILE";
}

/** Host without the www prefix; falls back to the raw string. */
export function hostOf(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, "");
  } catch {
    return url;
  }
}

export function joinPath(directory: string, filename: string): string {
  return `${directory.replace(/[\\/]+$/, "")}\\${filename}`;
}

export function plural(count: number, singular: string, many = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : many}`;
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
  return sanitizeFilename(name ?? "") || "download.bin";
}

/**
 * Make a server- or link-provided name safe for the target filesystem.
 * Strips directories, control characters, and Windows-ambiguous edges while
 * preserving Unicode and the original case.
 */
export function sanitizeFilename(value: string): string {
  const decoded = safeDecode(value);
  const base = decoded.replace(/\\/g, "/").split("/").filter(Boolean).pop() ?? "";
  const trimmed = base.replace(/[. ]+$/g, "").trim();
  if (!trimmed) return "";

  const dot = trimmed.lastIndexOf(".");
  const stem = dot > 0 ? trimmed.slice(0, dot) : trimmed;
  const extension = dot > 0 ? trimmed.slice(dot + 1).toLowerCase() : "";
  const cleanStem = sanitizeComponent(stem) || "download";
  const cleanExtension = extension && /^[a-z0-9]{1,12}$/.test(extension) ? `.${extension}` : "";
  const name = `${cleanStem}${cleanExtension}`;
  return /^\.+$/.test(name) ? "" : name;
}

function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

/** Characters Windows rejects in filenames. */
function sanitizeComponent(value: string): string {
  const cleaned = value.replace(/[<>:"/\\|?*\u0000-\u001f]+/g, " ").replace(/\s+/g, " ").trim();
  // Windows reserves these stems.
  const reserved = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\.|$)/i;
  if (reserved.test(cleaned)) return `_${cleaned}`;
  return cleaned;
}
