import type { Download } from "@/lib/types";
import type { ViewFilter } from "@/store/engine";

export interface ViewCopy {
  label: string;
  title: string;
  emptyTitle: string;
  emptyBody: string;
}

export const VIEW_COPY: Record<ViewFilter, ViewCopy> = {
  all: {
    label: "All downloads",
    title: "All downloads",
    emptyTitle: "No downloads yet",
    emptyBody:
      "Paste a direct link to start a transfer. Segment progress is recoverable, and an existing file is never overwritten.",
  },
  active: {
    label: "Active",
    title: "In progress",
    emptyTitle: "Nothing is moving",
    emptyBody:
      "Transfers appear here the moment they start, with their connection lanes, throughput, and time remaining.",
  },
  queued: {
    label: "Waiting",
    title: "Waiting",
    emptyTitle: "The queue is empty",
    emptyBody:
      "Downloads wait here while every active slot is busy, or when you add one paused on purpose.",
  },
  completed: {
    label: "Completed",
    title: "Completed",
    emptyTitle: "No files have finished",
    emptyBody:
      "Finished transfers stay here so you can open the file or show it in its folder.",
  },
  failed: {
    label: "Failed",
    title: "Failed",
    emptyTitle: "Nothing has failed",
    emptyBody:
      "A transfer that stops before completion appears here with whatever progress is safely on disk.",
  },
  history: {
    label: "History",
    title: "History",
    emptyTitle: "No history yet",
    emptyBody:
      "Completed, failed, and stopped transfers are kept here until you remove them.",
  },
};

export const NAV_GROUPS: Array<{ heading: string; views: ViewFilter[] }> = [
  { heading: "Transfers", views: ["all", "active", "queued"] },
  { heading: "Archive", views: ["completed", "failed", "history"] },
];

const ACTIVE: Download["state"][] = ["Downloading", "Pausing"];

export function matchesView(view: ViewFilter, download: Download): boolean {
  switch (view) {
    case "all":
      return true;
    case "active":
      return ACTIVE.includes(download.state);
    case "queued":
      return download.state === "Queued" || download.state === "Paused";
    case "completed":
      return download.state === "Completed";
    case "failed":
      return download.state === "Failed";
    case "history":
      return download.state === "Completed" || download.state === "Failed" || download.state === "Canceled";
  }
}

export function groupFor(view: ViewFilter): string {
  return view === "completed" || view === "failed" || view === "history" ? "Archive" : "Transfers";
}

export function matchesQuery(download: Download, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === "") return true;
  return (
    download.url.toLowerCase().includes(needle) ||
    download.destination.toLowerCase().includes(needle)
  );
}
