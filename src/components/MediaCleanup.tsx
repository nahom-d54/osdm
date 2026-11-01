import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Field, ErrorNote } from "@/components/ui/field";
import { getBridge } from "@/lib/bridge";
import type { MediaJob } from "@/lib/retrieval";

export function MediaCleanup({ jobs, onRemoved }: { jobs: MediaJob[]; onRemoved: () => Promise<void> }) {
  const [id, setId] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pending = useRef(false);
  const completed = jobs.filter((job) => job.state === "Completed");
  if (!completed.length) return null;
  return <fieldset disabled={busy} className="space-y-3 border-t border-line pt-4">
    <legend className="text-sm font-medium">Clean completed media inputs</legend>
    <p className="text-xs text-muted-foreground">Remove a completed job, its segment history, verified local segments and merge anchor. The published output is kept. Modified or unrelated files are refused. A tiny ownership marker is retained; no directory is recursively deleted. Interrupted cleanup can be retried.</p>
    <Field label="Completed job to clean" htmlFor="media-clean-job"><select id="media-clean-job" className="h-9 w-full rounded-md border border-line bg-panel px-3 text-sm" value={id} onChange={(event) => { setId(event.target.value); setConfirmed(false); }}><option value="">Choose a completed job</option>{completed.map((job) => <option key={job.id} value={job.id}>{job.removal_pending ? "Cleanup pending · " : ""}{job.destination}</option>)}</select></Field>
    <label className="flex items-start gap-2 text-sm"><input type="checkbox" checked={confirmed} onChange={(event) => setConfirmed(event.target.checked)} /><span>I authorize removal of this job's intermediate files and segment history, keeping its published output.</span></label>
    <Button size="sm" variant="outline" disabled={!confirmed || !completed.some((job) => job.id === id)} onClick={async () => {
      if (pending.current) return;
      pending.current = true;
      setBusy(true); setError(null);
      try { await getBridge().removeCompletedMediaJob(id, true); setId(""); setConfirmed(false); await onRemoved(); }
      catch (error) { setError(String(error)); }
      finally { pending.current = false; setBusy(false); }
    }}>Clean completed media job</Button>
    {error && <ErrorNote>{error}</ErrorNote>}
  </fieldset>;
}
