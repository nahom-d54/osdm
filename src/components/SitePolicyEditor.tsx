import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Field, ErrorNote } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { getBridge } from "@/lib/bridge";
import { DEFAULT_SITE_POLICY, utcSiteSchedule, type SiteProject } from "@/lib/site-retrieval";

/** Transient, explicit drafts; polling snapshots never overwrite in-progress edits. */
export function SitePolicyEditor({ project, onSaved, onClose }: {
  project: SiteProject; onSaved: () => Promise<void>; onClose: () => void;
}) {
  const original = project.policy ?? DEFAULT_SITE_POLICY;
  const [automatic, setAutomatic] = useState(original.automatic_downloads);
  const [cache, setCache] = useState(original.cache_responses);
  const [concurrent, setConcurrent] = useState(String(original.max_concurrent_downloads));
  const [at, setAt] = useState(original.next_run == null ? "" : new Date(original.next_run * 1000).toISOString().slice(0, 16));
  const [repeat, setRepeat] = useState(original.interval_seconds == null ? "" : String(original.interval_seconds / 60));
  const [catchUp, setCatchUp] = useState(original.catch_up_on_startup);
  const [consent, setConsent] = useState(false);
  const [extensions, setExtensions] = useState((project.extensions ?? []).join(" "));
  const [excludes, setExcludes] = useState((project.excluded_paths ?? []).join("\n"));
  const [filterConsent, setFilterConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pending = useRef(false);
  const first = useRef<HTMLInputElement>(null);
  useEffect(() => { first.current?.focus(); }, []);

  const save = async () => {
    if (pending.current) return;
    pending.current = true;
    setError(null); setBusy(true);
    try {
      if (!consent) throw new Error("Confirm the unattended project policy.");
      if (!/^\d+$/.test(concurrent) || Number(concurrent) < 1 || Number(concurrent) > 32) throw new Error("Choose between 1 and 32 active resource downloads.");
      const schedule = utcSiteSchedule(at, repeat);
      if (schedule.next_run != null && schedule.next_run <= Date.now() / 1000) throw new Error("Choose a future UTC time, or use Recheck now.");
      await getBridge().configureSitePolicy(project.id, { automatic_downloads: automatic, max_concurrent_downloads: Number(concurrent), cache_responses: cache,
        ...schedule, catch_up_on_startup: catchUp }, true);
      await onSaved(); onClose();
    } catch (error) { setError(String(error)); }
    finally { pending.current = false; setBusy(false); }
  };
  const updateFilters = async () => {
    if (pending.current) return;
    pending.current = true;
    setError(null); setBusy(true);
    try {
      if (!filterConsent) throw new Error("Confirm the filter changes.");
      await getBridge().updateSiteFilters(project.id, extensions.split(/\s+/).filter(Boolean), excludes.split(/\r?\n/).filter(Boolean), true);
      await onSaved(); onClose();
    } catch (error) { setError(String(error)); }
    finally { pending.current = false; setBusy(false); }
  };
  return <section aria-label="Site project settings" className="space-y-5 rounded-md border border-brand-line bg-panel p-4">
    <div className="flex items-center justify-between gap-2"><h4 className="text-sm font-medium">Project policy & continuation</h4><Button size="sm" variant="ghost" disabled={busy} onClick={onClose}>Close settings</Button></div>
    <fieldset disabled={busy} className="space-y-4"><legend className="mb-3 text-sm font-medium">Unattended retrieval</legend>
      <label className="flex items-start gap-2 text-sm"><input ref={first} type="checkbox" checked={automatic} onChange={(event) => { setAutomatic(event.target.checked); setConsent(false); }} /><span>Automatically queue matching, known-size resources after each exploration</span></label>
      <p className="text-xs text-muted-foreground">Off by default. Applies on the next exploration/recheck, inside the same approved scope and lifetime payload budget. Unknown or over-budget resources remain for review.</p>
      <Field label="Active resource downloads (1–32)" htmlFor="site-concurrency" hint="A ceiling for new admissions; existing transfers finish. Global and host engine ceilings still apply."><Input id="site-concurrency" inputMode="numeric" value={concurrent} maxLength={2} onChange={(event) => { setConcurrent(event.target.value); setConsent(false); }} /></Field>
      <label className="flex items-start gap-2 text-sm"><input type="checkbox" checked={cache} onChange={(event) => { setCache(event.target.checked); setConsent(false); }} /><span>Use a bounded, always-revalidated local page cache</span></label>
      <p className="text-xs text-muted-foreground">HTML only, with ETag validation. No stale-on-error fallback; robots and resource bodies are never cached. No-store/private responses are excluded.</p>
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Next recheck (UTC)" htmlFor="site-run-at" hint="Leave empty to disable scheduling. OSDM must be running."><Input id="site-run-at" type="datetime-local" value={at} onChange={(event) => { setAt(event.target.value); setConsent(false); }} /></Field>
        <Field label="Repeat every (minutes)" htmlFor="site-run-repeat" hint="1–525,600, or empty for one run."><Input id="site-run-repeat" inputMode="numeric" value={repeat} maxLength={6} onChange={(event) => { setRepeat(event.target.value); setConsent(false); }} /></Field>
      </div>
      <label className="flex items-start gap-2 text-sm"><input type="checkbox" checked={catchUp} onChange={(event) => { setCatchUp(event.target.checked); setConsent(false); }} /><span>Coalesce a missed run once after app restart</span></label>
      <p className="text-xs text-muted-foreground">Otherwise missed startup runs are skipped. Overlapping runs skip their slot; clock reversal cannot replay it. Rechecks publish separate revisions and never replace earlier files. Pause/Cancel disables the schedule.</p>
      <label className="flex items-start gap-2 rounded-md border border-brand-line bg-brand-soft p-3 text-sm"><input type="checkbox" checked={consent} onChange={(event) => setConsent(event.target.checked)} /><span>I authorize this unattended policy within the project's approved scope and lifetime budgets.</span></label>
      <Button size="sm" disabled={!consent} onClick={() => void save()}>Save project policy</Button>
    </fieldset>
    <fieldset disabled={busy} className="space-y-3 border-t border-line pt-4"><legend className="text-sm font-medium">Continue with changed filters</legend>
      <Field label="New allowed file extensions" htmlFor="site-edit-extensions"><Input id="site-edit-extensions" value={extensions} onChange={(event) => { setExtensions(event.target.value); setFilterConsent(false); }} /></Field>
      <Field label="New excluded path prefixes" htmlFor="site-edit-excludes"><textarea id="site-edit-excludes" className="w-full rounded-md border border-line bg-panel p-2 text-sm" rows={2} value={excludes} onChange={(event) => { setExcludes(event.target.value); setFilterConsent(false); }} /></Field>
      <p className="text-xs text-muted-foreground">Keeps all existing files and download history, then rechecks the original origin/path with the new filters. Lifetime accounting is preserved. Finish or cancel queued files first. The project is saved paused, and scheduling/automatic downloading are disabled until re-enabled.</p>
      <label className="flex items-start gap-2 text-sm"><input type="checkbox" checked={filterConsent} onChange={(event) => setFilterConsent(event.target.checked)} /><span>I confirm these filter changes without expanding the origin or directory scope.</span></label>
      <Button size="sm" variant="secondary" disabled={!filterConsent} onClick={() => void updateFilters()}>Save filters & pause</Button>
    </fieldset>
    {error && <ErrorNote>{error}</ErrorNote>}
  </section>;
}
