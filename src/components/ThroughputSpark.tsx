import { useEffect, useState } from "react";
import { useEngine } from "@/store/engine";

const SAMPLE_COUNT = 27;
const SAMPLE_MS = 600;

/** A quiet equalizer of the last sixteen seconds of aggregate throughput. */
export function ThroughputSpark() {
  const [samples, setSamples] = useState<number[]>(() => Array.from({ length: SAMPLE_COUNT }, () => 0));

  useEffect(() => {
    const sample = () => {
      const speed = useEngine
        .getState()
        .downloads.reduce((sum, download) => (download.state === "Downloading" ? sum + download.bytes_per_second : sum), 0);
      setSamples((previous) => [...previous.slice(1), speed]);
    };
    sample();
    const timer = window.setInterval(sample, SAMPLE_MS);
    return () => window.clearInterval(timer);
  }, []);

  const peak = Math.max(1, ...samples);

  return (
    <div className="flex h-8 items-end gap-[2px] border-b border-line-soft pb-[3px]" aria-hidden="true">
      {samples.map((value, index) => {
        const height = value <= 0 ? 1.5 : Math.max(2.5, (value / peak) * 28);
        return (
          <span
            key={index}
            className="flex-1 rounded-[1px] bg-brand transition-[height] duration-500 ease-out"
            style={{
              height: `${height}px`,
              opacity: 0.2 + (index / (SAMPLE_COUNT - 1)) * 0.8,
            }}
          />
        );
      })}
    </div>
  );
}
