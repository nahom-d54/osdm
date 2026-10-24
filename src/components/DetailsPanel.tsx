import type { Download } from "@/lib/types";
import { committedBytes } from "@/lib/types";
import { formatBytes, formatEta, formatSpeed } from "@/lib/format";
import { SegmentRibbon } from "@/components/SegmentRibbon";
import { Button, IconButton } from "@/components/ui/Button";
import { FolderIcon, OpenIcon, PauseIcon, PlayIcon, RetryIcon, XIcon, TrashIcon } from "@/components/icons";
import { useEngine } from "@/store/engine";

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[5.5rem_1fr] gap-2 py-1.5">
      <dt className="pt-px text-[11px] font-semibold tracking-[0.04em] text-faint uppercase">{label}</dt>
      <dd className="min-w-0 text-[12px] text-ink">{children}</dd>
    </div>
  );
}

function MonoValue({ value }: { value: string }) {
  return <span className="block break-all font-mono text-[11px] leading-relaxed text-muted">{value}</span>;
}

export function DetailsPanel({ download }: { download: Download | null }) {
  const { pause, resume, retry, cancel, openFile, revealFile, removeDownload } = useEngine();

  if (!download) {
    return (
      <aside className="flex w-80 shrink-0 items-center justify-center border-l border-line bg-panel">
        <p className="max-w-48 text-center text-[12px] leading-relaxed text-faint">
          Select a download to inspect its segments and errors.
        </p>
      </aside>
    );
  }

  const drained = download.state === "Paused" || download.state === "Completed" || download.state === "Failed" || download.state === "Canceled";

  return (
    <aside className="flex w-80 shrink-0 flex-col overflow-y-auto border-l border-line bg-panel" aria-label="Download details">
      <header className="flex items-start justify-between gap-2 border-b border-line-soft px-4 pt-4 pb-3">
        <h2 className="font-display text-[14px] font-semibold text-ink-strong" title={download.destination}>
          {fileName(download.destination)}
        </h2>
        <div className="flex shrink-0 gap-0.5">
          {download.state === "Downloading" && (
            <IconButton label="Pause" onClick={() => void pause(download.id)}>
              <PauseIcon size={12} />
            </IconButton>
          )}
          {download.state === "Paused" && (
            <IconButton label="Resume" onClick={() => void resume(download.id)}>
              <PlayIcon size={12} />
            </IconButton>
          )}
          {(download.state === "Failed" || download.state === "Canceled") && (
            <IconButton label="Retry" onClick={() => void retry(download.id)}>
              <RetryIcon size={12} />
            </IconButton>
          )}
          {download.state === "Downloading" && (
            <IconButton label="Stop" onClick={() => void cancel(download.id)}>
              <XIcon size={12} />
            </IconButton>
          )}
        </div>
      </header>

      <div className="flex flex-1 flex-col gap-4 px-4 py-3.5">
        {download.error && (
          <div className="rounded-[var(--radius-control)] bg-danger-soft p-2.5 text-[12px] leading-relaxed text-danger">
            {download.error}
            <span className="mt-1 block text-[11.5px] text-danger/80">
              {drained ? "All workers have drained; kept progress is committed." : "Stopping workers now."}
            </span>
          </div>
        )}

        <Section heading="Throughput">
          <div className="grid grid-cols-3 gap-2">
            <Stat label="Rate" value={download.bytes_per_second > 0 ? formatSpeed(download.bytes_per_second) : "—"} />
            <Stat label="Remaining" value={formatEta(download.eta_seconds)} />
            <Stat
              label="Committed"
              value={`${formatBytes(committedBytes(download))}${download.total_bytes ? ` / ${formatBytes(download.total_bytes)}` : ""}`}
            />
          </div>
        </Section>

        <Section heading="Segments">
          <div className="mb-2">
            <SegmentRibbon download={download} />
          </div>
          {download.segments.length > 1 ? (
            <table className="w-full font-mono text-[10.5px]" aria-label="Segment activity">
              <thead>
                <tr className="text-left text-faint">
                  <th scope="col" className="pb-1 font-normal">Range</th>
                  <th scope="col" className="pb-1 text-right font-normal">Synced</th>
                  <th scope="col" className="pb-1 text-right font-normal">Live</th>
                </tr>
              </thead>
              <tbody>
                {download.segments.map((seg, index) => {
                  const size = seg.end! - seg.start;
                  return (
                    <tr key={index} className="border-t border-line-soft text-muted">
                      <td className="py-1">
                        {formatBytes(seg.start)} → {formatBytes(seg.end!)}
                      </td>
                      <td className="py-1 text-right text-ok/90">{formatBytes(seg.committed)}</td>
                      <td className="py-1 text-right">{formatBytes(Math.max(0, seg.downloaded - seg.committed))}</td>
                      <td aria-hidden className="sr-only">
                        of {formatBytes(size)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          ) : (
            <p className="text-[11.5px] text-muted">
              Single stream{download.ranged ? " with range support" : " — no usable validator, no ranges"}.
            </p>
          )}
        </Section>

        <Section heading="Location">
          <dl>
            <Row label="Origin">
              <MonoValue value={download.url} />
            </Row>
            <Row label="Destination">
              <MonoValue value={download.destination} />
            </Row>
            <Row label="Temp file">
              <MonoValue value={download.temporary_path} />
            </Row>
            <Row label="Validator">
              {download.validator ? (
                <MonoValue value={download.validator} />
              ) : (
                <span className="text-[11.5px] text-faint">none — server provided no validator</span>
              )}
            </Row>
          </dl>
        </Section>

        {download.state === "Completed" && (
          <div className="mt-auto flex gap-2 pt-2">
            <Button variant="primary" className="flex-1 justify-center" onClick={() => void openFile(download.id)}>
              <OpenIcon size={12} /> Open
            </Button>
            <Button className="flex-1 justify-center" onClick={() => void revealFile(download.id)}>
              <FolderIcon size={12} /> Show in folder
            </Button>
          </div>
        )}

        {["Completed", "Failed", "Canceled"].includes(download.state) && (
          <Button
            variant="danger"
            className="justify-center"
            onClick={() => void removeDownload(download.id, true)}
          >
            <TrashIcon size={12} /> Delete entry and partial file
          </Button>
        )}
      </div>
    </aside>
  );
}

function Section({ heading, children }: { heading: string; children: React.ReactNode }) {
  return (
    <section>
      <h3 className="mb-2 text-[10px] font-semibold tracking-[0.08em] text-faint uppercase">{heading}</h3>
      {children}
    </section>
  );
}

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="rounded-[var(--radius-control)] border border-line-soft bg-raise px-2 py-1.5">
      <div className="text-[10px] tracking-[0.06em] text-faint uppercase">{label}</div>
      <div className="mt-0.5 font-mono text-[11.5px] text-ink">{value}</div>
    </div>
  );
}

function fileName(path: string): string {
  const cleaned = path.replace(/\\/g, "/");
  return cleaned.slice(cleaned.lastIndexOf("/") + 1) || path;
}
