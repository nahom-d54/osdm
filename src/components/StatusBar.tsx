import { useEngine } from "@/store/engine";
import { formatSpeed } from "@/lib/format";
import { GaugeIcon } from "@/components/icons";
import { useSettings } from "@/store/settings";

const SPEED_OPTIONS = [
  { bytes: 0, label: "No limit" },
  { bytes: 1 * 1024 * 1024, label: "1 MB/s" },
  { bytes: 5 * 1024 * 1024, label: "5 MB/s" },
  { bytes: 20 * 1024 * 1024, label: "20 MB/s" },
];

/** The measurement strip: aggregate throughput at all times, plus counts and limit. */
export function StatusBar() {
  const { downloads, applySpeedLimit } = useEngine();
  const { settings, update } = useSettings();

  const throughput = downloads
    .filter((d) => d.state === "Downloading")
    .reduce((sum, d) => sum + d.bytes_per_second, 0);
  const activeCount = downloads.filter((d) => d.state === "Downloading" || d.state === "Pausing").length;
  const queuedCount = downloads.filter((d) => d.state === "Queued" || d.state === "Paused").length;

  return (
    <footer className="flex h-9 shrink-0 items-center gap-4 border-t border-line bg-panel px-3 font-mono text-[11px]">
      <span className="flex items-center gap-1.5 text-ok" aria-live="polite">
        <GaugeIcon size={12} aria-hidden="true" />
        {throughput > 0 ? formatSpeed(throughput) : "0 B/s"}
      </span>
      <span className="text-muted">
        {activeCount} active · {queuedCount} queued
      </span>

      <span className="ml-auto flex items-center gap-2 text-muted">
        <label htmlFor="speed-limit">Speed limit</label>
        <select
          id="speed-limit"
          value={settings.speed_limit_bytes_per_second}
          onChange={(event) => {
            const bytes = Number(event.target.value);
            update({ speed_limit_bytes_per_second: bytes });
            void applySpeedLimit(bytes);
          }}
          className="h-6 rounded-[var(--radius-control)] border border-line bg-raise px-1.5 font-mono text-[11px] text-ink focus:outline-none focus-visible:border-accent"
        >
          {SPEED_OPTIONS.map((option) => (
            <option key={option.bytes} value={option.bytes}>
              {option.label}
            </option>
          ))}
        </select>
      </span>
    </footer>
  );
}
