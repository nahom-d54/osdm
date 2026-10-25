import type { Download } from "./types";

export type DownloadSort = "queue" | "file" | "state" | "rate" | "eta" | "size";

/** Presentation only: never changes the engine's durable queue order. */
export function orderDownloads(
  downloads: Download[],
  sort: DownloadSort,
  descending: boolean,
): Download[] {
  if (sort === "queue") return downloads;
  const value = (download: Download): string | number | null => {
    switch (sort) {
      case "file":
        return download.destination.split(/[\\/]/).pop() ?? "";
      case "state":
        return download.state;
      case "rate":
        return download.state === "Downloading"
          ? download.bytes_per_second
          : null;
      case "eta":
        return download.state === "Downloading" ? download.eta_seconds : null;
      case "size":
        return download.total_bytes;
    }
  };
  return [...downloads].sort((a, b) => {
    const left = value(a),
      right = value(b);
    // Unknown values remain last in either direction; stable ties retain queue order.
    if (left == null) return right == null ? 0 : 1;
    if (right == null) return -1;
    const comparison =
      typeof left === "string" && typeof right === "string"
        ? left.localeCompare(right, undefined, {
            numeric: true,
            sensitivity: "base",
          })
        : Number(left) - Number(right);
    return descending ? -comparison : comparison;
  });
}
