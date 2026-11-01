import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { ErrorNote, Field } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Modal } from "@/components/ui/modal";
import { getBridge } from "@/lib/bridge";
import type { Download } from "@/lib/types";

/** The capture is already durably paused. No probe, enqueue or start on open. */
export function BrowserCaptureDialog({ download, onReviewed }: { download: Download; onReviewed: (id: string) => void }) {
  const [destination, setDestination] = useState(download.destination);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pending = useRef(false);
  async function review(action: "start" | "later" | "discard", saveDestination = true) {
    if (pending.current) return;
    pending.current = true; setBusy(true); setError(null);
    try {
      if (saveDestination && action !== "discard" && destination !== download.destination) await getBridge().updateDestination(download.id, destination);
      await getBridge().reviewCapture(download.id, action);
      onReviewed(download.id);
    } catch { setError("Could not finish this review. Check the destination and queue before retrying."); }
    finally { pending.current = false; setBusy(false); }
  }
  return <Modal open onOpenChange={(open) => { if (!open && !pending.current) void review("later", false); }} title="Download captured from browser" description="Nothing is downloading yet. Choose where to save it, then start or keep it for later." footer={<div className="flex flex-wrap justify-end gap-2"><Button variant="ghost" disabled={busy} onClick={() => void review("discard")}>Discard capture</Button><Button variant="outline" disabled={busy || !destination.trim()} onClick={() => void review("later")}>Download later</Button><Button disabled={busy || !destination.trim()} onClick={() => void review("start")}>Start download</Button></div>}>
    <div className="space-y-4"><Field label="Source URL" htmlFor="browser-capture-source"><Input id="browser-capture-source" value={download.url} readOnly className="font-mono text-xs" /></Field><Field label="Save as" htmlFor="browser-capture-destination"><Input id="browser-capture-destination" value={destination} disabled={busy} onChange={(event) => setDestination(event.target.value)} /></Field><p className="text-xs text-muted-foreground">Closing keeps this capture paused. Browser login cookies remain in memory for this session only.</p>{error && <ErrorNote>{error}</ErrorNote>}</div>
  </Modal>;
}
