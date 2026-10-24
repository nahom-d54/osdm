import { useEffect, useState } from "react";
import { Dialog } from "@/components/ui/Dialog";
import { Button } from "@/components/ui/Button";
import { Field, TextField, ErrorNote } from "@/components/ui/TextField";
import { suggestFilename } from "@/lib/format";
import { getBridge } from "@/lib/bridge";
import { useEngine } from "@/store/engine";
import { useSettings } from "@/store/settings";
import { FolderIcon } from "@/components/icons";

interface AddDownloadDialogProps {
  open: boolean;
  onClose: () => void;
}

/**
 * Add-download flow: URL, destination picker, filename suggestion, and
 * conflict handling. The engine never overwrites, so the dialog never
 * submits into an existing file.
 */
export function AddDownloadDialog({ open, onClose }: AddDownloadDialogProps) {
  const { enqueue } = useEngine();
  const { settings, update } = useSettings();
  const [url, setUrl] = useState("");
  const [directory, setDirectory] = useState("");
  const [filename, setFilename] = useState("");
  const [conflict, setConflict] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Reset on every open.
  useEffect(() => {
    if (!open) return;
    setUrl("");
    setDirectory(settings.default_directory || "C:\\Users\\you\\Downloads");
    setFilename("");
    setConflict(false);
    setError(null);
    setBusy(false);
  }, [open, settings.default_directory]);

  // Suggest a filename from the URL until the user types their own.
  useEffect(() => {
    if (!open || url === "") return;
    setFilename(suggestFilename(url.trim()));
  }, [url, open]);

  const fullPath = (): string | null => {
    if (!directory || !filename) return null;
    return `${directory.replace(/[\\/]+$/, "")}\\${filename}`;
  };

  // Conflict check whenever the complete target path changes.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    const target = fullPath();
    if (target == null) {
      setConflict(false);
      return;
    }
    void getBridge()
      .pathExists(target)
      .then((exists) => {
        if (!cancelled) setConflict(exists);
      });
    return () => {
      cancelled = true;
    };
  });

  const urlValid = /^https?:\/\/\S+/.test(url.trim());

  const submit = async () => {
    const target = fullPath();
    if (!urlValid) {
      setError("Enter an http or https URL.");
      return;
    }
    if (target == null) {
      setError("Choose a destination folder and filename.");
      return;
    }
    if (await getBridge().pathExists(target)) {
      setConflict(true);
      return;
    }
    setBusy(true);
    try {
      await enqueue(url.trim(), target);
      update({ default_directory: directory });
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  const pickFolder = async () => {
    const picked = await getBridge().pickDirectory("Choose download folder", directory || undefined);
    if (picked) setDirectory(picked);
  };

  /** Offer "name (2).ext" as a copy name so the next check passes. */
  const saveCopy = () => {
    const dot = filename.lastIndexOf(".");
    const base = dot > 0 ? filename.slice(0, dot) : filename;
    const ext = dot > 0 ? filename.slice(dot) : "";
    setFilename(`${base} (2)${ext}`);
  };

  return (
    <Dialog open={open} title="Add download" onClose={onClose} width="33rem">
      <div className="flex flex-col gap-4 px-5 py-4">
        <Field
          label="URL"
          hint="Direct http or https link. The probe decides between segmented and single-stream transfer."
        >
          <TextField
            autoFocus
            placeholder="https://example.com/setup.iso"
            value={url}
            invalid={url !== "" && !urlValid}
            onChange={(event) => {
              setUrl(event.target.value);
              setError(null);
            }}
          />
        </Field>

        <Field label="Destination folder">
          <div className="flex gap-2">
            <TextField value={directory} readOnly tabIndex={-1} className="flex-1 cursor-default" aria-label="Destination folder" />
            <Button onClick={() => void pickFolder()}>
              <FolderIcon size={13} />
              Folder
            </Button>
          </div>
        </Field>

        <Field label="Filename">
          <TextField
            value={filename}
            invalid={conflict}
            onChange={(event) => setFilename(event.target.value)}
          />
        </Field>

        {conflict && (
          <ErrorNote>
            A file already exists at <span className="font-mono">{fullPath()}</span>. The engine never
            overwrites — choose another name, or{" "}
            <button type="button" onClick={saveCopy} className="font-semibold underline">
              use a copy name
            </button>
            .
          </ErrorNote>
        )}
        {error && <ErrorNote>{error}</ErrorNote>}

        <div className="flex items-center justify-between gap-3 border-t border-line-soft pt-3">
          <span className="min-w-0 truncate font-mono text-[11px] text-faint">
            {fullPath() ?? "Choose a folder and filename"}
          </span>
          <Button variant="primary" disabled={!urlValid || conflict || busy || !filename} onClick={() => void submit()}>
            {busy ? "Adding…" : "Add download"}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
