import { useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Download,
  Folder,
  FolderOpen,
  Link2,
  LoaderCircle,
  TriangleAlert,
} from "lucide-react";
import { cn } from "cn";

import { FileBadge } from "@/components/FileBadge";
import { Button } from "@/components/ui/button";
import { ErrorNote, Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Modal } from "@/components/ui/modal";
import { getBridge } from "@/lib/bridge";
import { formatBytes, hostOf, joinPath, sanitizeFilename, suggestFilename } from "@/lib/format";
import type { UrlProbe } from "@/lib/types";
import { useEngine } from "@/store/engine";
import { useSettings } from "@/store/settings";

interface AddDownloadDialogProps {
  open: boolean;
  onClose: () => void;
}

type Step = "source" | "confirm";
type NameSource = "server" | "link";

const INVALID_FILENAME = /[<>:"/\\|?*\u0000-\u001f]/;
const RESERVED_STEM = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/i;

/**
 * Two-step add flow. The engine probes the link first, then the UI asks the
 * user to confirm the real server filename and the destination path.
 */
export function AddDownloadDialog({ open, onClose }: AddDownloadDialogProps) {
  const { enqueue } = useEngine();
  const { settings, update } = useSettings();
  const [step, setStep] = useState<Step>("source");
  const [url, setUrl] = useState("");
  const [directory, setDirectory] = useState("");
  const [filename, setFilename] = useState("");
  const [probe, setProbe] = useState<UrlProbe | null>(null);
  const [nameSource, setNameSource] = useState<NameSource>("link");
  const [probeError, setProbeError] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [inspecting, setInspecting] = useState(false);
  const probeSequence = useRef(0);

  // Reset on every open, and invalidate any in-flight probe on close.
  useEffect(() => {
    probeSequence.current += 1;
    if (!open) return;
    setStep("source");
    setUrl("");
    setDirectory(settings.default_directory);
    setFilename("");
    setProbe(null);
    setNameSource("link");
    setProbeError(null);
    setConflict(false);
    setError(null);
    setBusy(false);
    setInspecting(false);

    let cancelled = false;
    const useFallback = async () => {
      const fallback = await getBridge().defaultDownloadDirectory();
      if (!cancelled) setDirectory((current) => current || fallback);
    };
    if (settings.default_directory === "") {
      void useFallback().catch(() => {});
    } else {
      void getBridge()
        .pathExists(settings.default_directory)
        .then((exists) => {
          if (cancelled || exists) return;
          setDirectory("");
          return useFallback();
        })
        .catch(() => {});
    }
    return () => {
      cancelled = true;
    };
  }, [open, settings.default_directory]);

  const trimmedUrl = url.trim();
  const urlValid = /^https?:\/\/\S+$/i.test(trimmedUrl);
  const stem = filename.includes(".") ? filename.slice(0, filename.lastIndexOf(".")) : filename;
  const filenameInvalid =
    filename !== "" &&
    (INVALID_FILENAME.test(filename) ||
      filename.trim() !== filename ||
      /[. ]$/.test(filename) ||
      filename === "." ||
      filename === ".." ||
      RESERVED_STEM.test(stem));
  const target = directory && filename ? joinPath(directory, filename) : null;

  useEffect(() => {
    if (!open || step !== "confirm" || target == null) {
      setConflict(false);
      return;
    }
    let cancelled = false;
    setConflict(false);
    void getBridge()
      .pathExists(target)
      .then((exists) => {
        if (!cancelled) setConflict(exists);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [open, step, target]);

  const inspect = async () => {
    if (!urlValid) {
      setError("Enter a direct http:// or https:// link.");
      return;
    }
    if (inspecting) return;
    const sequence = ++probeSequence.current;
    setInspecting(true);
    setError(null);
    setProbeError(null);

    try {
      const result = await getBridge().probe(trimmedUrl);
      if (sequence !== probeSequence.current) return;
      const folder = directory || (await getBridge().defaultDownloadDirectory().catch(() => ""));
      if (sequence !== probeSequence.current) return;
      const serverName = result.filename ? sanitizeFilename(result.filename) : "";
      setProbe(result);
      setDirectory(folder);
      setFilename(serverName || suggestFilename(result.final_url || trimmedUrl));
      setNameSource(serverName ? "server" : "link");
      setStep("confirm");
    } catch (caught) {
      if (sequence !== probeSequence.current) return;
      const folder = directory || (await getBridge().defaultDownloadDirectory().catch(() => ""));
      if (sequence !== probeSequence.current) return;
      setProbe({
        final_url: trimmedUrl,
        filename: null,
        content_type: null,
        total_bytes: null,
        ranged: false,
      });
      setDirectory(folder);
      setFilename(suggestFilename(trimmedUrl));
      setNameSource("link");
      setProbeError(errorMessage(caught));
      setStep("confirm");
    } finally {
      if (sequence === probeSequence.current) setInspecting(false);
    }
  };

  const download = async () => {
    if (target == null) {
      setError("Choose a destination folder and a filename.");
      return;
    }
    if (filenameInvalid) {
      setError("The filename contains characters Windows cannot store.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (await getBridge().pathExists(target)) {
        setConflict(true);
        setBusy(false);
        return;
      }
      await enqueue(trimmedUrl, target);
      try {
        update({ default_directory: directory });
      } catch {
        // The download is already queued; a preference write is not critical.
      }
      onClose();
    } catch (caught) {
      setError(errorMessage(caught));
      setBusy(false);
    }
  };

  const pickFolder = async () => {
    try {
      const picked = await getBridge().pickDirectory("Choose download folder", directory || undefined);
      if (picked) setDirectory(picked);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const saveCopy = () => {
    const dot = filename.lastIndexOf(".");
    const base = dot > 0 ? filename.slice(0, dot) : filename;
    const extension = dot > 0 ? filename.slice(dot) : "";
    const existing = base.match(/^(.*) \((\d+)\)$/);
    const next = existing ? `${existing[1]} (${Number(existing[2]) + 1})` : `${base} (2)`;
    setFilename(`${next}${extension}`);
  };

  const canDownload =
    urlValid &&
    directory !== "" &&
    filename !== "" &&
    !filenameInvalid &&
    !conflict &&
    !busy;

  const sourceHint = inspecting
    ? "Reading the filename, size, and range support from the server…"
    : "Direct HTTP or HTTPS link. Redirects are followed.";
  const filenameHint =
    nameSource === "server"
      ? "Named by the server in its Content-Disposition header."
      : probeError
        ? "The link could not be inspected. This name comes from the link."
        : "The server did not name this file. This name comes from the link.";

  return (
    <Modal
      open={open}
      onOpenChange={(next) => {
        if (!next && busy) return;
        if (!next) onClose();
      }}
      title={step === "source" ? "Add download" : "Download file"}
      description={
        step === "source"
          ? "Paste a direct link. OSDM asks the server for its filename before the transfer starts."
          : "Review the filename and destination. The transfer starts only when you press Download."
      }
      className="sm:max-w-[34rem]"
      footer={
        step === "source" ? (
          <div className="flex w-full items-center justify-end gap-2">
            <Button variant="ghost" size="sm" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" form="add-source-form" size="sm" disabled={!urlValid || inspecting}>
              {inspecting ? (
                <LoaderCircle className="animate-spin motion-reduce:animate-none" />
              ) : (
                <ArrowRight />
              )}
              {inspecting ? "Inspecting…" : "Continue"}
            </Button>
          </div>
        ) : (
          <div className="flex w-full items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              className="mr-auto"
              disabled={busy}
              onClick={() => {
                setStep("source");
                setError(null);
                setConflict(false);
              }}
            >
              <ArrowLeft />
              Back
            </Button>
            <Button variant="ghost" size="sm" disabled={busy} onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" form="add-confirm-form" size="sm" disabled={!canDownload}>
              {busy ? <LoaderCircle className="animate-spin motion-reduce:animate-none" /> : <Download />}
              {busy ? "Starting…" : probeError ? "Download anyway" : "Download"}
            </Button>
          </div>
        )
      }
    >
      <div key={step} className="row-in">
        {step === "source" ? (
          <form
            id="add-source-form"
            aria-busy={inspecting}
            className="flex flex-col gap-4"
            onSubmit={(event) => {
              event.preventDefault();
              void inspect();
            }}
          >
            <Field label="Source URL" htmlFor="download-url" hint={sourceHint}>
              <div className="relative">
                <Link2
                  className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-faint"
                  strokeWidth={1.75}
                  aria-hidden="true"
                />
                <Input
                  id="download-url"
                  autoFocus
                  disabled={inspecting}
                  autoCapitalize="off"
                  spellCheck={false}
                  placeholder="https://example.com/archive.tar.zst"
                  value={url}
                  invalid={url !== "" && !urlValid}
                  onChange={(event) => {
                    setUrl(event.target.value);
                    setError(null);
                  }}
                  className="h-10 pl-9 font-mono text-[12px]"
                />
              </div>
            </Field>
            {error && <ErrorNote>{error}</ErrorNote>}
            <p className="font-mono text-[9px] uppercase tracking-[0.08em] text-faint">
              The server name is preferred; the link name is the fallback.
            </p>
          </form>
        ) : (
          <form
            id="add-confirm-form"
            aria-busy={busy}
            className="flex flex-col gap-4"
            onSubmit={(event) => {
              event.preventDefault();
              void download();
            }}
          >
            {probeError && (
              <div
                role="status"
                className="flex items-start gap-2 rounded-md border border-warn/20 bg-warn-soft/35 px-3 py-2 text-[10.5px] leading-relaxed text-warn"
              >
                <TriangleAlert className="mt-px size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
                <span>
                  Couldn't read a filename from the server ({probeError}). Using the link name; the
                  transfer may still work.
                </span>
              </div>
            )}

            <div className="rounded-lg border border-line-soft bg-raise/40 p-3">
              <div className="flex items-start gap-3">
                <FileBadge name={filename || "download.bin"} size="lg" />
                <div className="min-w-0 flex-1">
                  <p
                    className="truncate font-display text-[16px] font-semibold leading-tight tracking-[-0.02em] text-ink-strong"
                    title={filename || undefined}
                  >
                    {filename || "Untitled file"}
                  </p>
                  <p
                    className="mt-1 truncate font-mono text-[9.5px] text-faint"
                    title={probe?.final_url || trimmedUrl}
                  >
                    {hostOf(probe?.final_url || trimmedUrl)}
                  </p>
                </div>
              </div>
              <dl className="mt-3 grid grid-cols-3 divide-x divide-line-soft border-t border-line-soft pt-3">
                <Info
                  label="Size"
                  value={probe?.total_bytes != null ? formatBytes(probe.total_bytes) : "Unknown"}
                />
                <Info
                  label="Type"
                  value={typeLabel(probe?.content_type ?? null)}
                  title={probe?.content_type ?? "Unknown"}
                  className="pl-3"
                />
                <Info
                  label="Ranges"
                  value={probeError ? "Unknown" : probe?.ranged ? "Supported" : "Single stream"}
                  className="pl-3"
                />
              </dl>
            </div>

            <Field
              label="Filename"
              htmlFor="download-filename"
              hint={filenameHint}
              trailing={<NameSourceTag source={nameSource} />}
            >
              <Input
                id="download-filename"
                autoFocus
                disabled={busy}
                value={filename}
                invalid={filenameInvalid || conflict}
                onChange={(event) => {
                  setFilename(event.target.value);
                  setError(null);
                }}
                className="font-mono text-[12px]"
                placeholder="archive.tar.zst"
              />
            </Field>

            <Field label="Save to" htmlFor="download-folder">
              <div className="flex gap-2">
                <Input
                  id="download-folder"
                  readOnly
                  tabIndex={-1}
                  value={directory}
                  placeholder="Choose a folder"
                  aria-label="Destination folder"
                  className="flex-1 cursor-default font-mono text-[11px] text-muted-foreground"
                />
                <Button
                  variant="secondary"
                  disabled={busy}
                  onClick={() => void pickFolder()}
                  className="shrink-0"
                >
                  <FolderOpen strokeWidth={1.75} />
                  Choose…
                </Button>
              </div>
              <p
                className="mt-1.5 flex items-center gap-1.5 truncate font-mono text-[10px] text-muted-foreground"
                title={target ?? undefined}
              >
                <Folder className="size-3 shrink-0 text-faint" strokeWidth={1.75} aria-hidden="true" />
                {target ?? "Choose a destination folder"}
              </p>
            </Field>

            {conflict && (
              <ErrorNote>
                A file already exists at{" "}
                <span className="break-all font-mono text-[10.5px]">{target}</span>. Choose another
                name, or{" "}
                <button type="button" onClick={saveCopy} className="font-semibold underline underline-offset-2">
                  use a copy name
                </button>
                .
              </ErrorNote>
            )}
            {filenameInvalid && (
              <ErrorNote>The filename contains characters Windows cannot store.</ErrorNote>
            )}
            {error && <ErrorNote>{error}</ErrorNote>}
          </form>
        )}
      </div>
    </Modal>
  );
}

function NameSourceTag({ source }: { source: NameSource }) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 font-mono text-[8.5px] uppercase tracking-[0.1em]",
        source === "server" ? "text-ok" : "text-faint",
      )}
    >
      <span className={cn("size-1 rounded-full", source === "server" ? "bg-ok" : "bg-faint")} />
      {source === "server" ? "server name" : "from link"}
    </span>
  );
}

function Info({ label, value, title, className }: { label: string; value: string; title?: string; className?: string }) {
  return (
    <div className={cn("min-w-0", className)}>
      <dt className="eyebrow">{label}</dt>
      <dd className="mt-1 truncate font-mono text-[10px] text-muted-foreground" title={title ?? value}>
        {value}
      </dd>
    </div>
  );
}

const TYPE_LABELS: Record<string, string> = {
  "application/zip": "ZIP archive",
  "application/x-iso9660-image": "ISO image",
  "application/pdf": "PDF document",
  "application/x-msi": "Windows installer",
  "application/vnd.microsoft.portable-executable": "Windows app",
  "application/octet-stream": "Binary file",
  "video/mp4": "MP4 video",
  "video/x-matroska": "Matroska video",
  "application/zstd": "Zstandard archive",
};

function typeLabel(value: string | null): string {
  if (!value) return "Unknown";
  return TYPE_LABELS[value.toLowerCase()] ?? value;
}

function errorMessage(value: unknown): string {
  return value instanceof Error ? value.message : String(value);
}
