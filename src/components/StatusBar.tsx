import { cn } from "cn";

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { formatBytes, formatSpeed } from "@/lib/format";
import { useEngine } from "@/store/engine";
import { useSettings } from "@/store/settings";

const SPEED_OPTIONS = [
  { value: 0, label: "No limit" },
  { value: 1024 * 1024, label: "1 MB/s" },
  { value: 5 * 1024 * 1024, label: "5 MB/s" },
  { value: 20 * 1024 * 1024, label: "20 MB/s" },
];

/** The measurement strip: engine health, aggregate rate, and the live limit. */
export function StatusBar() {
  const { downloads, applySpeedLimit, engineError, ready } = useEngine();
  const { settings, update } = useSettings();

  const throughput = downloads.reduce(
    (sum, download) => (download.state === "Downloading" ? sum + download.bytes_per_second : sum),
    0,
  );
  const active = downloads.filter((download) => download.state === "Downloading").length;
  const waiting = downloads.filter((download) => download.state === "Queued" || download.state === "Paused").length;
  const limit = settings.speed_limit_bytes_per_second;
  const status = engineError ? "Engine fault" : ready ? "Engine ready" : "Engine starting";

  const options = SPEED_OPTIONS.some((option) => option.value === limit)
    ? SPEED_OPTIONS
    : [...SPEED_OPTIONS, { value: limit, label: `${formatBytes(limit)}/s` }];

  return (
    <footer className="flex h-8 shrink-0 items-center gap-3 border-t border-line-soft bg-panel px-4 font-mono text-[10px]">
      <span className="flex items-center gap-2" title={engineError ?? "The local engine is ready"}>
        <span
          aria-hidden="true"
          className={cn(
            "size-1.5 rounded-full",
            engineError ? "bg-danger" : ready ? "bg-ok" : "breathe bg-faint",
          )}
        />
        <span className="text-[8.5px] uppercase tracking-[0.12em] text-faint">{status}</span>
      </span>

      <span aria-hidden="true" className="h-3 w-px bg-line" />

      <span className="text-muted-foreground tnum">{throughput > 0 ? formatSpeed(throughput) : "Idle"}</span>
      <span className="text-faint tnum">
        {active} active · {waiting} waiting
      </span>

      <div className="ml-auto flex items-center gap-2">
        <span className="text-[8.5px] uppercase tracking-[0.12em] text-faint">Limit</span>
        <Select
          items={options}
          value={limit}
          onValueChange={(value) => {
            if (value == null) return;
            update({ speed_limit_bytes_per_second: value });
            void applySpeedLimit(value);
          }}
        >
          <SelectTrigger
            size="sm"
            aria-label="Speed limit"
            className="border-line-soft bg-raise/60 text-[10.5px]"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {options.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    </footer>
  );
}
