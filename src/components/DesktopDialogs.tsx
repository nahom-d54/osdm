import { useEffect, useRef, useState } from "react";
import { Modal } from "@/components/ui/modal";
import { Button } from "@/components/ui/button";
import { SegmentRibbon } from "@/components/SegmentRibbon";
import { fileNameOf, formatSpeed } from "@/lib/format";
import { useEngine } from "@/store/engine";
import { useSettings } from "@/store/settings";
import { getBridge } from "@/lib/bridge";

export function DesktopDialogs() {
  const { downloads, ready, pause, openFile, revealFile } = useEngine();
  const preferences = useSettings((state) => state.settings.desktop);
  const previous = useRef<Map<string, string> | null>(null);
  const [completed, setCompleted] = useState<string[]>([]);
  const [progress, setProgress] = useState<string | null>(null);
  useEffect(() => {
    if (!ready) return;
    const before = previous.current;
    previous.current = new Map(
      downloads.map((download) => [download.id, download.state]),
    );
    if (!before) return; // Never replay historical completions after startup.
    const done = downloads.filter(
      (download) =>
        download.state === "Completed" &&
        before.has(download.id) &&
        before.get(download.id) !== "Completed",
    );
    setCompleted((ids) => {
      if (preferences.silent || !preferences.completion_dialog)
        return ids.length ? [] : ids;
      const next = [
        ...ids.filter((id) => downloads.some((download) => download.id === id)),
        ...done.map((download) => download.id),
      ];
      return next.length === ids.length &&
        next.every((id, index) => id === ids[index])
        ? ids
        : next;
    });
    const added = downloads.find(
      (download) =>
        !before.has(download.id) &&
        ["Queued", "Downloading"].includes(download.state),
    );
    if (added && preferences.progress_dialog && !preferences.silent)
      setProgress(added.id);
  }, [downloads, ready, preferences]);
  const complete = downloads.find((download) => download.id === completed[0]);
  const active = downloads.find(
    (download) =>
      download.id === progress &&
      ["Queued", "Downloading", "Pausing"].includes(download.state),
  );
  const dismiss = () => setCompleted((ids) => ids.slice(1));
  const progressBody = active && (
    <div className="flex flex-col gap-3">
      <p className="truncate text-sm">{fileNameOf(active.destination)}</p>
      <SegmentRibbon download={active} />
      <p className="text-xs">
        {active.state} · {formatSpeed(active.bytes_per_second)}
      </p>
      <Button
        size="sm"
        variant="secondary"
        onClick={() => void pause(active.id)}
      >
        Pause
      </Button>
    </div>
  );
  return (
    <>
      <Modal
        open={
          !!complete && preferences.completion_dialog && !preferences.silent
        }
        onOpenChange={(open) => {
          if (!open) dismiss();
        }}
        title="Download completed"
        description="The file has been published. A computed hash alone is not proof of authenticity."
        footer={
          <>
            <Button variant="ghost" onClick={dismiss}>
              Close
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                if (complete) void revealFile(complete.id);
              }}
            >
              Show in folder
            </Button>
            <Button
              onClick={() => {
                if (complete) void openFile(complete.id);
              }}
            >
              Open file
            </Button>
          </>
        }
      >
        <p className="break-all text-sm">{complete?.destination}</p>
        {complete && (
          <Button
            variant="secondary"
            className="mt-3"
            onPointerDown={(event) => {
              if (event.button === 0)
                void getBridge()
                  .dragCompleted(complete.id)
                  .catch((error) =>
                    useEngine.setState({ banner: String(error) }),
                  );
            }}
          >
            Drag file out
          </Button>
        )}
        <p className="mt-2 text-xs">
          {complete?.integrity === "Verified"
            ? "SHA-256 matches the supplied reference."
            : "No trusted checksum match is established unless shown as Verified."}
        </p>
      </Modal>
      {preferences.minimized_progress && active && !preferences.silent ? (
        <aside
          aria-label="Download progress"
          className="fixed right-4 bottom-12 z-40 w-72 rounded-md border border-line-soft bg-panel p-4 shadow-lg"
        >
          {progressBody}
          <Button size="sm" variant="ghost" onClick={() => setProgress(null)}>
            Hide progress
          </Button>
        </aside>
      ) : (
        <Modal
          open={!!active && preferences.progress_dialog && !preferences.silent}
          onOpenChange={(open) => {
            if (!open) setProgress(null);
          }}
          title="Download progress"
          description="You can hide this dialog without stopping the download."
          footer={
            <Button onClick={() => setProgress(null)}>Hide progress</Button>
          }
        >
          {progressBody}
        </Modal>
      )}
    </>
  );
}
