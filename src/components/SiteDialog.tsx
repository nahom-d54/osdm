import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Field, ErrorNote } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Modal } from "@/components/ui/modal";
import { getBridge } from "@/lib/bridge";
import { formatBytes } from "@/lib/format";
import { mediaSource } from "@/lib/retrieval";
import { scopeFor, siteLimits, type SiteProject, type SiteAction } from "@/lib/site-retrieval";
import { useSettings } from "@/store/settings";
import { SitePolicyEditor } from "@/components/SitePolicyEditor";
import { useEngine } from "@/store/engine";

const STEPS = ["Source", "Scope", "Filters & budgets", "Review resources"];

export function SiteDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const defaultDirectory = useSettings((state) => state.settings.default_directory);
  const downloads = useEngine((state) => state.downloads);
  const [step, setStep] = useState(0);
  const [name, setName] = useState("");
  const [seed, setSeed] = useState("");
  const [prefix, setPrefix] = useState("/");
  const [destination, setDestination] = useState("");
  const [extensions, setExtensions] = useState("pdf html htm png jpg svg css");
  const [excludes, setExcludes] = useState("");
  const [pages, setPages] = useState("20");
  const [files, setFiles] = useState("100");
  const [mebibytes, setMebibytes] = useState("64");
  const [depth, setDepth] = useState("3");
  const [confirmed, setConfirmed] = useState(false);
  const [projects, setProjects] = useState<SiteProject[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const [policyOpen, setPolicyOpen] = useState(false);
  const generation = useRef(0);
  const admitting = useRef(false);
  const content = useRef<HTMLDivElement>(null);
  const policyButton = useRef<HTMLButtonElement>(null);
  const project = projects.find((value) => value.id === projectId);
  useEffect(() => { setSelected([]); }, [projectId, project?.generation]);

  useEffect(() => {
    if (!open) return;
    const first = content.current?.querySelector<HTMLElement>("input:not([disabled]), textarea:not([disabled]), select:not([disabled]), button:not([disabled])");
    (first ?? content.current)?.focus();
  }, [step, open, projectId]);

  useEffect(() => {
    if (!open) return;
    let disposed = false;
    let pending = false;
    setError(null); setNotice("");
    setPolicyOpen(false);
    setDestination((value) => value || defaultDirectory);
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try { const projects = await getBridge().siteProjects(); if (!disposed) setProjects(projects); }
      catch (error) { if (!disposed) setError(String(error)); }
      finally { pending = false; }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 1000);
    return () => { generation.current += 1; disposed = true; clearInterval(timer); };
  }, [open, defaultDirectory]);

  const next = () => {
    setError(null);
    try {
      if (step === 0) {
        mediaSource(seed.trim());
        if (!name.trim() || !destination.trim()) throw new Error("Name the project and choose an output folder.");
        setPrefix(scopeFor(seed));
      }
      if (step === 1) {
        const source = new URL(mediaSource(seed.trim()));
        if (!prefix.startsWith("/") || !prefix.endsWith("/") || !source.pathname.startsWith(prefix) || prefix.includes("?") || prefix.includes("#")) throw new Error("Use an absolute directory path ending in / that contains the seed page.");
      }
      setStep(step + 1);
    } catch (error) { setError(String(error)); }
  };
  const explore = async () => {
    if (admitting.current) return;
    admitting.current = true;
    const current = generation.current;
    setBusy(true); setError(null); setNotice("");
    try {
      const limits = siteLimits({ pages, files, mebibytes, depth });
      if (!confirmed) throw new Error("Confirm this site's scope and budgets before exploring.");
      const id = await getBridge().createSiteProject({ name: name.trim(), seed_url: mediaSource(seed.trim()), path_prefix: prefix, destination, extensions: extensions.split(/\s+/).filter(Boolean), excluded_paths: excludes.split(/\r?\n/).filter(Boolean), ...limits, confirmed });
      // Save project & explore is explicit admission. A saved exploration keeps
      // running when its dialog closes, until its Pause/Cancel control is used.
      if (current !== generation.current) return;
      setProjectId(id); setSelected([]); setStep(3);
      await getBridge().controlSiteProject(id, "resume");
      const projects = await getBridge().siteProjects();
      if (current === generation.current) setProjects(projects);
    } catch (error) { if (current === generation.current) setError(String(error)); }
    finally { admitting.current = false; setBusy(false); }
  };
  const control = async (action: SiteAction) => {
    if (!project) return;
    setBusy(true); setError(null);
    try {
      await getBridge().controlSiteProject(project.id, action);
      setProjects(await getBridge().siteProjects());
      if (action === "delete") { setProjectId(null); setStep(0); setNotice("Project deleted. Downloaded files and existing download entries are kept."); }
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  };
  const queue = async () => {
    if (!project) return;
    setBusy(true); setError(null); setNotice("");
    try {
      const ids = await getBridge().queueSiteResources(project.id, selected);
      setProjects(await getBridge().siteProjects()); setSelected([]);
      setNotice(`${ids.length} resources added to the main download list. Existing files are never replaced.`);
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  };
  return <Modal open={open} onOpenChange={(value) => { if (!value) onClose(); }} title="Site Grabber" description="Explore a bounded part of one website, then choose what to save. Robots restrictions apply; no login automation or JavaScript execution." className="sm:max-w-[48rem]" footer={<>
    <Button variant="ghost" onClick={onClose}>Close</Button>
    {step > 0 && step < 3 && <Button variant="secondary" disabled={busy} onClick={() => setStep(step - 1)}>Back</Button>}
    {step < 2 && <Button onClick={next}>Continue</Button>}
    {step === 2 && <Button disabled={busy || !confirmed} onClick={() => void explore()}>Save project & explore</Button>}
    {step === 3 && <Button disabled={busy || selected.length === 0 || project?.state !== 'Ready'} onClick={() => void queue()}>Queue {selected.length || "selected"} resources</Button>}
  </>}><div ref={content} tabIndex={-1} className="space-y-5">
    <ol className="grid grid-cols-4 gap-2 border-b border-line pb-4 text-[10px]">{STEPS.map((label, index) => <li key={label} aria-current={step === index ? "step" : undefined} className={step === index ? "font-medium text-brand" : "text-muted-foreground"}><span className="mb-1 block font-mono">{index + 1}/4</span>{label}</li>)}</ol>
    {step === 0 && <>
      <Field label="Project name" htmlFor="site-name"><Input id="site-name" value={name} maxLength={128} onChange={(event) => setName(event.target.value)} /></Field>
      <Field label="Starting page URL" htmlFor="site-seed"><Input id="site-seed" value={seed} onChange={(event) => { setSeed(event.target.value); setConfirmed(false); }} spellCheck={false} autoComplete="off" /></Field>
      <Field label="Output folder" htmlFor="site-output"><div className="flex gap-2"><Input id="site-output" value={destination} onChange={(event) => setDestination(event.target.value)} /><Button variant="outline" onClick={async () => { try { const result = await getBridge().pickDirectory("Site project output", destination); if (result) setDestination(result); } catch (error) { setError(String(error)); } }}>Browse</Button></div></Field>
      {projects.length > 0 && <section className="border-t border-line pt-4"><h3 className="mb-2 text-sm font-medium">Saved projects</h3>{projects.map((project) => <Button key={project.id} variant="ghost" className="mb-1 w-full justify-between" onClick={() => { setProjectId(project.id); setSelected([]); setStep(3); }}>{project.name}<span className="text-xs text-muted-foreground">{project.state}</span></Button>)}</section>}
    </>}
    {step === 1 && <>
      <p className="break-all text-sm">Only <span className="font-mono">{(() => { try { return new URL(seed).origin; } catch { return "the selected origin"; } })()}</span> may be explored. Links and redirects cannot expand this scope.</p>
      <Field label="Allowed directory path" htmlFor="site-prefix" hint="For example /manual/. Scope changes require your explicit confirmation."><Input id="site-prefix" value={prefix} onChange={(event) => { setPrefix(event.target.value); setConfirmed(false); }} /></Field>
      <p className="text-xs text-muted-foreground">Robots policy is checked first. Cross-origin links, embedded credentials and HTTPS downgrades are refused. No cookies or browser credentials are sent.</p>
    </>}
    {step === 2 && <>
      <Field label="Resource template"><div className="flex flex-wrap gap-2">{[['Documents', 'pdf html htm css png jpg svg'], ['Images', 'png jpg jpeg gif webp svg'], ['Audio', 'mp3 m4a ogg opus wav flac']] .map(([label, filter]) => <Button key={label} size="sm" variant="outline" onClick={() => setExtensions(filter)}>{label}</Button>)}</div></Field>
      <Field label="Allowed file extensions" htmlFor="site-extensions"><Input id="site-extensions" value={extensions} onChange={(event) => setExtensions(event.target.value)} /></Field>
      <Field label="Excluded path prefixes" htmlFor="site-excludes" hint="One absolute path per line."><textarea id="site-excludes" rows={2} className="w-full rounded-md border border-line bg-panel p-2 text-sm" value={excludes} onChange={(event) => setExcludes(event.target.value)} /></Field>
      <div className="grid grid-cols-2 gap-4">{[['Pages (1–1,000)', 'pages', pages, setPages], ['Files (1–10,000)', 'files', files, setFiles], ['Total MiB (1–10,240)', 'mib', mebibytes, setMebibytes], ['Link depth (1–16)', 'depth', depth, setDepth]].map(([label, id, value, setter]) => <Field key={id as string} label={label as string} htmlFor={`site-${id}`}><Input id={`site-${id}`} inputMode="numeric" value={value as string} onChange={(event) => (setter as (value: string) => void)(event.target.value)} /></Field>)}</div>
      <p className="text-xs text-muted-foreground">The total is a payload/storage budget: discovery bodies plus selected file sizes. Retry traffic is not a metered-network quota.</p>
      <label className="flex items-start gap-3 rounded-md border border-brand-line bg-brand-soft p-3 text-sm"><input type="checkbox" className="mt-1" checked={confirmed} onChange={(event) => setConfirmed(event.target.checked)} /><span>I authorize anonymous exploration only within this origin, directory, filters and finite budgets. I will review discovered resources before downloading.</span></label>
    </>}
    {step === 3 && project && <>
      <div className="flex flex-wrap items-center justify-between gap-3"><div><h3 className="font-medium">{project.name}</h3><p className="mt-1 font-mono text-xs text-muted-foreground">{project.state} · {project.pages} pages · {formatBytes(project.bytes)} accounted</p></div><div className="flex gap-2"><Button size="sm" variant="outline" disabled={busy} onClick={() => void control(project.state === 'Exploring' ? 'pause' : 'resume')}>{project.state === 'Exploring' ? 'Pause exploration' : 'Resume exploration'}</Button><Button size="sm" variant="ghost" disabled={busy} onClick={() => void control('cancel')}>Cancel</Button></div></div>
      {project.error && <ErrorNote>{project.error}</ErrorNote>}
      {project.resources.length === 0 ? <p className="text-sm text-muted-foreground">No matching resources discovered yet. Resume exploration or use the filter editor to continue within this scope.</p> : <fieldset className="rounded-md border border-line"><legend className="px-2 text-sm">Review resources · unknown sizes cannot be queued</legend><div className="max-h-72 divide-y divide-line-soft overflow-auto">{project.resources.map((resource) => {
        const admitted = resource.download_id != null && downloads.some((download) => download.id === resource.download_id);
        return <label key={resource.id} className="flex items-start gap-3 p-3 text-sm"><input type="checkbox" className="mt-1" disabled={resource.size == null || admitted || project.state !== 'Ready'} checked={selected.includes(resource.id)} onChange={(event) => setSelected((current) => event.target.checked ? [...current, resource.id] : current.filter((id) => id !== resource.id))} /><span className="min-w-0 flex-1"><span className="block break-all">{resource.description || resource.relative_path}</span>{resource.description && <span className="block break-all font-mono text-[10px] text-muted-foreground">{resource.relative_path}</span>}<span className="text-xs text-muted-foreground">{admitted ? "Already queued" : resource.download_id ? "Admission pending · select to retry" : resource.size == null ? "Size unknown · not queued" : formatBytes(resource.size)}</span></span></label>;
      })}</div></fieldset>}
      <div className="space-y-2 text-xs text-muted-foreground"><p>Revision {project.generation ?? 0} · {project.policy?.automatic_downloads ? 'Automatic matching downloads enabled' : 'Manual review'} · Up to {project.policy?.max_concurrent_downloads ?? 1} active resource downloads</p>{project.policy?.next_run != null && <p>Next recheck · {new Date(project.policy.next_run * 1000).toISOString().replace('T', ' ').replace('.000Z', ' UTC')}</p>}{project.schedule_note && <p role="status">{project.schedule_note}</p>}<p>Rechecks keep previous copies and count new revisions against the same lifetime payload budget.</p></div>
      <div className="flex flex-wrap gap-2"><Button size="sm" variant="outline" disabled={busy || project.state === 'Exploring'} onClick={() => void control('recheck')}>Recheck now</Button><Button ref={policyButton} size="sm" variant="outline" disabled={busy} onClick={() => setPolicyOpen((value) => !value)}>Schedule, automation & filters</Button></div>
      {policyOpen && <SitePolicyEditor key={project.id} project={project} onClose={() => { setPolicyOpen(false); policyButton.current?.focus(); }} onSaved={async () => { setProjects(await getBridge().siteProjects()); setNotice('Project policy saved. Existing files are kept.'); }} />}
      <div className="flex flex-wrap justify-between gap-2 border-t border-line pt-3"><Button size="sm" variant="secondary" onClick={() => { setStep(0); setProjectId(null); setConfirmed(false); setPolicyOpen(false); }}>New project</Button><Button size="sm" variant="ghost" disabled={busy || project.state === 'Exploring'} onClick={() => void control('delete')}>Delete saved project · keep files</Button></div>
    </>}
    {step === 3 && !project && <p className="text-sm text-muted-foreground">Loading the saved project…</p>}
    {error && <ErrorNote>{error}</ErrorNote>}{notice && <p role="status" className="text-sm text-ok">{notice}</p>}
  </div></Modal>;
}
