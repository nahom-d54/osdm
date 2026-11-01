import { useEffect, useRef, useState } from "react";
import { FolderOpen, LoaderCircle, Pause, Play, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Field, ErrorNote } from "@/components/ui/field";
import { Modal } from "@/components/ui/modal";
import { getBridge } from "@/lib/bridge";
import { joinPath, sanitizeFilename } from "@/lib/format";
import { authorizedOrigins, mediaSource, qualityLabel, type MediaInspection, type MediaJob, type MediaOffer } from "@/lib/retrieval";
import { useSettings } from "@/store/settings";
import { MediaCleanup } from "@/components/MediaCleanup";

export function MediaDialog({ open, onClose, offer }: { open: boolean; onClose: () => void; offer: MediaOffer | null }) {
  const settings = useSettings((state) => state.settings);
  const [source, setSource] = useState("");
  const [mode, setMode] = useState<"hls" | "dash" | "direct">("hls");
  const [subtitle, setSubtitle] = useState("");
  const [origins, setOrigins] = useState("");
  const [inspection, setInspection] = useState<MediaInspection | null>(null);
  const [inspectedSource, setInspectedSource] = useState("");
  const [selected, setSelected] = useState("");
  const [dashVideo, setDashVideo] = useState("");
  const [dashAudio, setDashAudio] = useState("");
  const [directory, setDirectory] = useState("");
  const [filename, setFilename] = useState("video.mkv");
  const [ffmpeg, setFfmpeg] = useState("");
  const [description, setDescription] = useState("");
  const [category, setCategory] = useState("");
  const [jobs, setJobs] = useState<MediaJob[]>([]);
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const sequence = useRef(0);
  const activeInspection = useRef<string | null>(null);

  const cancelInspection = () => {
    sequence.current += 1;
    if (activeInspection.current) void getBridge().cancelMediaInspection(activeInspection.current).catch(() => {});
    activeInspection.current = null;
  };
  useEffect(() => {
    if (!open) return;
    let disposed = false;
    setSource(offer?.url ?? "");
    setMode(offer?.kind === "direct" ? "direct" : offer?.kind === "dash" || /\.mpd(?:[?#]|$)/i.test(offer?.url ?? "") ? "dash" : "hls");
    setSubtitle(offer?.subtitle_url ?? "");
    setFilename(`${sanitizeFilename(offer?.title || "video")}.mkv`);
    setDescription(offer?.title ?? "");
    setCategory(""); setDashVideo(""); setDashAudio("");
    setDirectory(settings.default_directory);
    setInspection(null); setInspectedSource(""); setSelected(""); setOrigins(""); setError(null); setNotice(""); setBusy(false);
    if (!settings.default_directory) void getBridge().defaultDownloadDirectory().then((value) => { if (!disposed) setDirectory((current) => current || value); }).catch(() => {});
    return () => { disposed = true; cancelInspection(); };
  }, [open, offer]);

  useEffect(() => {
    if (!open) return;
    let disposed = false;
    let pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try { const result = await getBridge().mediaJobs(); if (!disposed) setJobs(result); }
      catch (error) { if (!disposed) setError(String(error)); }
      finally { pending = false; }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 1000);
    return () => { disposed = true; clearInterval(timer); };
  }, [open]);

  const inspect = async (value: string) => {
    cancelInspection();
    const current = sequence.current;
    setError(null); setInspection(null); setBusy(true);
    const requestId = crypto.randomUUID();
    activeInspection.current = requestId;
    try {
      const url = mediaSource(value.trim());
      const result = await getBridge().inspectMedia(url, authorizedOrigins(origins), requestId, mode === "dash" ? "dash" : "hls");
      if (current !== sequence.current) return;
      setSource(url); setInspectedSource(url); setInspection(result); setSelected(""); setDashVideo(""); setDashAudio("");
    } catch (error) { if (current === sequence.current) setError(String(error)); }
    finally { if (current === sequence.current) { activeInspection.current = null; setBusy(false); } }
  };

  const add = async () => {
    if (savingRef.current) return;
    savingRef.current = true;
    setSaving(true);
    const current = sequence.current;
    setError(null); setNotice(""); setBusy(true);
    try {
      const url = mediaSource(source.trim());
      const name = sanitizeFilename(filename);
      if (!directory || !name || name !== filename.trim()) throw new Error("Choose an output folder and a safe filename.");
      const destination = joinPath(directory, name);
      if (await getBridge().pathExists(destination)) throw new Error("That output already exists. Choose a different filename.");
      // Closing/changing the dialog during native preflight cancels admission,
      // not merely its success message. Once submitted, the saved job is owned
      // by the engine and has explicit Pause/Cancel controls.
      if (current !== sequence.current) return;
      if (mode === "direct" && !subtitle.trim()) {
        await getBridge().enqueue({ url, destination, expected_sha256: null }, false, { description, category_id: category || null });
      } else {
        if (mode !== "direct" && (inspectedSource !== url || (mode === "hls" ? inspection?.kind !== "vod" : inspection?.kind !== "dash" || (!dashVideo && !dashAudio)))) throw new Error("Inspect and explicitly select a complete media stream first.");
        if (!ffmpeg.trim()) throw new Error("Enter the absolute path to an installed FFmpeg executable.");
        await getBridge().createMediaJob({ url, destination, allowed_origins: authorizedOrigins(origins), ffmpeg_path: ffmpeg.trim(), description: description || null, category_id: category || null, subtitle_url: subtitle.trim() ? mediaSource(subtitle.trim()) : null, dash_tracks: mode === "dash" ? [dashVideo, dashAudio].filter(Boolean) : [], direct_media: mode === "direct" });
        const jobs = await getBridge().mediaJobs();
        if (current === sequence.current) setJobs(jobs);
      }
      if (offer) await getBridge().dismissMediaOffer(offer.id);
      if (current === sequence.current) setNotice(mode === "direct" && !subtitle.trim() ? "Media added to downloads." : "Media job saved. Source transfers appear in the main list; final output appears after merging.");
    } catch (error) { if (current === sequence.current) setError(String(error)); }
    finally { savingRef.current = false; setSaving(false); if (current === sequence.current) setBusy(false); }
  };

  return <Modal open={open} onOpenChange={(value) => { if (!value) { cancelInspection(); onClose(); } }} title="Save media" description="Choose one item and its quality. No site-wide capture, DRM removal, or automatic playlist downloads." className="sm:max-w-[48rem]" footer={<>
    <Button variant="ghost" onClick={() => { cancelInspection(); onClose(); }}>Close</Button>
    <Button disabled={saving || busy || !directory || !filename || (mode === "hls" && (inspection?.kind !== "vod" || source !== inspectedSource)) || (mode === "dash" && (inspection?.kind !== "dash" || source !== inspectedSource || (!dashVideo && !dashAudio)))} onClick={() => void add()}>{busy && <LoaderCircle className="animate-spin" aria-hidden="true" />}Save selected media</Button>
  </>}>
    <fieldset disabled={saving} className="space-y-5"><legend className="sr-only">Media source and output</legend>
      <fieldset className="flex flex-wrap gap-5 text-sm"><legend className="mb-2 font-medium">Media type</legend>{([['hls', 'HLS stream'], ['dash', 'DASH stream'], ['direct', 'Direct media file']] as const).map(([value, label]) => <label key={value} className="flex items-center gap-2"><input type="radio" name="media-type" value={value} checked={mode === value} onChange={() => { cancelInspection(); setBusy(false); setMode(value); setInspection(null); }} />{label}</label>)}</fieldset>
      {mode === "dash" && <p className="text-xs text-muted-foreground">Static DASH with explicit segment lists is supported. Choose video and audio separately; no representation is automatically included. Live, DRM and template/timeline manifests are rejected.</p>}
      <Field label="Media URL" htmlFor="media-url"><Input id="media-url" value={source} onChange={(event) => { cancelInspection(); setBusy(false); setSource(event.target.value); setInspection(null); }} placeholder="https://…" autoComplete="off" spellCheck={false} /></Field>
      {mode === "direct" && !!subtitle.trim() && <Field label="Additional approved origins" htmlFor="media-origins" hint="Approve a separate subtitle origin explicitly. No credentials are forwarded."><textarea id="media-origins" className="w-full rounded-md border border-line bg-panel px-3 py-2 font-mono text-xs" rows={2} value={origins} onChange={(event) => setOrigins(event.target.value)} spellCheck={false} /></Field>}
      {mode !== "direct" && <>
        <Field label="Additional approved origins" htmlFor="media-origins" hint="One origin per line, such as https://cdn.example.org. No cookies or authorization are forwarded."><textarea id="media-origins" className="w-full rounded-md border border-line bg-panel px-3 py-2 font-mono text-xs" rows={2} value={origins} onChange={(event) => { cancelInspection(); setBusy(false); setOrigins(event.target.value); setInspection(null); }} spellCheck={false} /></Field>
        <Button variant="secondary" disabled={busy || !source.trim()} onClick={() => void inspect(source)}>{busy && <LoaderCircle className="animate-spin" aria-hidden="true" />}Inspect manifest</Button>
        {inspection?.kind === "master" && <fieldset className="rounded-md border border-line"><legend className="px-2 text-sm font-medium">Choose a quality</legend><div className="divide-y divide-line-soft">{inspection.choices.map((choice, index) => <label key={index} className="flex cursor-pointer items-center gap-3 p-3 text-sm"><input type="radio" name="media-quality" checked={selected === choice.url} onChange={() => setSelected(choice.url)} /><span className="min-w-0"><span className="block font-mono text-xs">{qualityLabel(choice)}</span><span className="block truncate text-xs text-muted-foreground">{new URL(choice.url).host}{choice.codecs ? ` · ${choice.codecs}` : ""}</span></span></label>)}</div><div className="border-t border-line-soft p-3"><Button size="sm" disabled={!selected || busy} onClick={() => void inspect(selected)}>Inspect selected quality</Button></div></fieldset>}
        {inspection?.kind === "vod" && <div role="status" className="rounded-md border border-brand-line bg-brand-soft p-3 text-sm"><p>{inspection.segment_count} ordered segments in this complete stream.</p><p className="mt-1 text-xs text-muted-foreground">Referenced origins: {inspection.resource_origins.join(", ")}. Approve any additional origins above, then inspect again.</p></div>}
        {inspection?.kind === "dash" && <div className="space-y-3">{(['video', 'audio'] as const).map((kind) => <fieldset key={kind} className="rounded-md border border-line"><legend className="px-2 text-sm">Choose {kind}</legend><div className="divide-y divide-line-soft"><label className="flex items-center gap-3 p-3 text-sm"><input type="radio" name={`dash-${kind}`} checked={!(kind === 'video' ? dashVideo : dashAudio)} onChange={() => (kind === 'video' ? setDashVideo : setDashAudio)("")} />No {kind} track</label>{inspection.choices.filter((choice) => choice.track_kind === kind).map((choice) => <label key={choice.track_key} className="flex items-center gap-3 p-3 text-sm"><input type="radio" name={`dash-${kind}`} checked={(kind === 'video' ? dashVideo : dashAudio) === choice.track_key} onChange={() => (kind === 'video' ? setDashVideo : setDashAudio)(choice.track_key ?? "")} /><span className="min-w-0"><span className="block font-mono text-xs">{qualityLabel(choice)}</span><span className="block text-xs text-muted-foreground">{choice.language ? `${choice.language} · ` : ""}{choice.codecs ?? "Codec not declared"} · {choice.segment_count} segments</span></span></label>)}</div></fieldset>)}
          {inspection.choices.filter((choice) => choice.track_kind === "subtitle" && choice.segment_count === 1 && choice.url !== source).map((choice) => <Button key={choice.track_key} size="sm" variant="outline" onClick={() => setSubtitle(choice.url)}>Use {choice.language || 'manifest'} subtitle</Button>)}
          <p className="text-xs text-muted-foreground">Referenced origins: {inspection.resource_origins.join(", ")}. Approve additional origins above and inspect again before saving.</p></div>}
      </>}
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Output folder" htmlFor="media-directory"><div className="flex gap-2"><Input id="media-directory" value={directory} onChange={(event) => setDirectory(event.target.value)} /><Button variant="outline" size="icon" aria-label="Choose media output folder" onClick={async () => { try { const result = await getBridge().pickDirectory("Save media", directory); if (result) setDirectory(result); } catch (error) { setError(String(error)); } }}><FolderOpen aria-hidden="true" /></Button></div></Field>
        <Field label="Filename" htmlFor="media-filename" hint={mode !== "direct" || subtitle.trim() ? "Use .mkv or .mp4 for merged output. Existing files are never replaced." : "Use the source file's media extension."}><Input id="media-filename" value={filename} onChange={(event) => setFilename(event.target.value)} /></Field>
      </div>
      <Field label="Description" htmlFor="media-description"><Input id="media-description" value={description} maxLength={4096} onChange={(event) => setDescription(event.target.value)} /></Field>
      <Field label="Category" htmlFor="media-category"><select id="media-category" className="h-9 rounded-md border border-line bg-panel px-3 text-sm" value={category} onChange={(event) => setCategory(event.target.value)}><option value="">General</option>{settings.categories.map((category) => <option key={category.id} value={category.id}>{category.name}</option>)}</select></Field>
      <Field label="Optional selected subtitle URL" htmlFor="media-subtitle" hint="SRT or WebVTT. This selection is passed to stream capture, never silently discarded. Clear it to save without subtitles."><Input id="media-subtitle" value={subtitle} onChange={(event) => setSubtitle(event.target.value)} autoComplete="off" spellCheck={false} /></Field>
      {mode === "direct" && subtitle.trim() && <p className="text-xs text-muted-foreground">Direct subtitle remux supports exact MP4/M4V/MOV sources without redirects. Other direct media can be saved without remux.</p>}
      {(mode !== "direct" || !!subtitle.trim()) && <>
        <Field label="Merged container" htmlFor="media-container"><select id="media-container" className="h-9 rounded-md border border-line bg-panel px-3 text-sm" value={/\.mp4$/i.test(filename) ? 'mp4' : 'mkv'} onChange={(event) => setFilename(`${filename.replace(/\.(mkv|mp4)$/i, '')}.${event.target.value}`)}><option value="mkv">Matroska (.mkv)</option><option value="mp4">MP4 (.mp4)</option></select></Field>
        <Field label="Installed FFmpeg executable" htmlFor="media-ffmpeg" hint="Absolute local path. OSDM does not install codecs or allow FFmpeg to fetch media over the network."><Input id="media-ffmpeg" value={ffmpeg} onChange={(event) => setFfmpeg(event.target.value)} placeholder="C:\Tools\ffmpeg\bin\ffmpeg.exe" spellCheck={false} /></Field>
      </>}
      {error && <ErrorNote>{error}</ErrorNote>}{notice && <p role="status" className="text-sm text-ok">{notice}</p>}
      {jobs.length > 0 && <section aria-label="Saved media jobs" className="border-t border-line pt-4"><h3 className="mb-3 text-sm font-medium">Saved media jobs</h3><div className="space-y-3">{jobs.map((job) => <div key={job.id} className="rounded-md border border-line-soft p-3"><p className="break-all text-sm">{job.destination}</p><p className="mt-1 font-mono text-xs text-muted-foreground">{job.state} · {job.completed_segments}/{job.total_segments} segments complete</p>{job.error && <p role="alert" className="mt-1 text-xs text-danger">{job.error}</p>}{job.sha256 && <p className="mt-1 break-all font-mono text-[10px] text-muted-foreground">Computed SHA-256 · {job.sha256}</p>}<div className="mt-2 flex gap-2">{(['Paused', 'Failed', 'Canceled'].includes(job.state)) && <Button size="sm" variant="outline" disabled={busy} onClick={async () => { try { await getBridge().controlMediaJob(job.id, "resume"); setJobs(await getBridge().mediaJobs()); } catch (error) { setError(String(error)); } }}><Play aria-hidden="true" />Resume</Button>}{['Downloading', 'Merging', 'Publishing'].includes(job.state) && <Button size="sm" variant="outline" disabled={busy} onClick={async () => { try { await getBridge().controlMediaJob(job.id, "pause"); setJobs(await getBridge().mediaJobs()); } catch (error) { setError(String(error)); } }}><Pause aria-hidden="true" />Pause</Button>}{job.state !== 'Completed' && job.state !== 'Canceled' && <Button size="sm" variant="ghost" disabled={busy} onClick={async () => { try { await getBridge().controlMediaJob(job.id, "cancel"); setJobs(await getBridge().mediaJobs()); } catch (error) { setError(String(error)); } }}><X aria-hidden="true" />Cancel job</Button>}</div></div>)}</div></section>}
      <p className="text-xs text-muted-foreground">Closing cancels inspection and unsent preflight. Once saved, a job continues in the engine until you pause or cancel it.</p>
      <MediaCleanup jobs={jobs} onRemoved={async () => { setJobs(await getBridge().mediaJobs()); setNotice("Media inputs cleaned. Published output kept."); }} />
    </fieldset>
  </Modal>;
}
