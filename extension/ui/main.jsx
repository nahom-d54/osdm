import React, { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { createRoot } from "react-dom/client";
import { ArrowDownToLine, Film, SlidersHorizontal, Cable, Check, Moon, Sun, ArrowUpRight, ShieldCheck } from "lucide-react";
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/familjen-grotesk/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import { captureDefaults } from "../defaults.js";
import { cookiePermission } from "../cookies.js";
import { mediaAllowed, mediaDefaults, mediaSite, toggleMediaSite } from "../media.js";
import "./style.css";

const api = globalThis.browser ?? globalThis.chrome;
const defaults = { ...captureDefaults, ...mediaDefaults, floatingVideos: false, theme: "system" };
const routes = [["capture", "Capture", ArrowDownToLine], ["media", "Video", Film], ["rules", "Rules", SlidersHorizontal], ["connection", "Connect", Cable]];
const subscribeHash = (notify) => { addEventListener("hashchange", notify); return () => removeEventListener("hashchange", notify); };
function useHashRoute() {
  const hash = useSyncExternalStore(subscribeHash, () => location.hash, () => "");
  const page = hash.replace(/^#\/?/, "");
  return routes.some(([route]) => route === page) ? page : "capture";
}
function Switch({ label, checked, onChange, disabled, children }) {
  return <label className="switch-row"><span><strong>{label}</strong>{children && <small>{children}</small>}</span><input type="checkbox" role="switch" aria-label={label} checked={checked} disabled={disabled} onChange={(event) => onChange(event.target.checked)} /></label>;
}
function App() {
  const route = useHashRoute();
  const [settings, setSettings] = useState(defaults);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  const [connected, setConnected] = useState(null);
  const [tab, setTab] = useState(null);
  const [lastCapture, setLastCapture] = useState(null);
  const [cookieAccess, setCookieAccess] = useState(null);
  const [offers, setOffers] = useState([]);
  const [selected, setSelected] = useState(null);
  const [subtitle, setSubtitle] = useState("");
  const [extensions, setExtensions] = useState("");
  const [exclusions, setExclusions] = useState("");
  const pending = useRef(false);
  const heading = useRef(null);
  useEffect(() => {
    let alive = true;
    async function load() {
      const values = await api.storage.local.get(defaults);
      const [active] = await api.tabs.query({ active: true, currentWindow: true });
      const { lastCapture } = await api.storage.session.get({ lastCapture: null });
      const permitted = await api.permissions.contains(cookiePermission("all"));
      if (alive) { setCookieAccess(permitted); setSettings(values); setExtensions(values.extensions); setExclusions(values.exclusions); setTab(active); setLastCapture(lastCapture); setLoaded(true); }
    }
    void load().catch(() => { if (alive) setError("Could not load extension settings. Reopen the extension and try again."); });
    return () => { alive = false; };
  }, []);
  useEffect(() => { setNotice(""); setError(""); heading.current?.focus(); }, [route]);
  useEffect(() => {
    const query = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => { document.documentElement.dataset.theme = settings.theme === "system" ? query.matches ? "dark" : "light" : settings.theme; };
    apply(); query.addEventListener("change", apply); return () => query.removeEventListener("change", apply);
  }, [settings.theme]);
  async function save(patch) { await api.storage.local.set(patch); setSettings((current) => ({ ...current, ...patch })); }
  async function act(operation) {
    if (pending.current) return;
    pending.current = true; setBusy(true); setError(""); setNotice("");
    try { await operation(); } catch { setError("That action could not finish. Check browser permissions and the OSDM connection, then retry."); }
    finally { pending.current = false; setBusy(false); }
  }
  async function enableCapture(enabled) {
    // Optional permissions must be requested from this user gesture, before
    // native I/O. There is no per-site approval list or background prompting.
    if (enabled && settings.cookiesEnabled && !await api.permissions.request(cookiePermission("all"))) { setError("Cookie permission was declined. Allow it, or turn off login cookie sharing below to capture without cookies."); return; }
    if (enabled && settings.cookiesEnabled) setCookieAccess(true);
    await save({ enabled }); setNotice(enabled ? "Automatic capture enabled. Keep OSDM open with local integration enabled." : "Capture paused. Browser downloads stay in the browser.");
  }
  async function shareCookies(value) {
    if (value && !await api.permissions.request(cookiePermission("all"))) { setError("Cookie permission was declined."); return; }
    if (value) setCookieAccess(true);
    await save({ cookiesEnabled: value });
  }
  async function disableSite(value) {
    const current = await api.storage.local.get({ mediaDisabledSites: [] });
    await save({ mediaDisabledSites: toggleMediaSite(current.mediaDisabledSites, tab.url, value) });
    setOffers([]); await api.storage.session.remove("networkMedia");
  }
  async function scan() {
    setOffers([]); setSelected(null); setSubtitle("");
    const reply = await api.runtime.sendMessage({ op: "scan_media" });
    if (!reply?.ok) { setError("This page cannot be scanned. Open a normal web page and enable media detection."); return; }
    setOffers(reply.offers); setNotice(reply.offers.length ? "Choose a source. Download quality is reviewed in OSDM." : "No HTTP media found. Enable network discovery, reload the page and start playback.");
  }
  async function network(enabled) {
    if (enabled && !await api.permissions.request({ permissions: ["webRequest"], origins: ["http://*/*", "https://*/*"] })) { setError("Permission declined. Page scanning is still available."); return; }
    await save({ networkMediaEnabled: enabled });
    await api.storage.session.remove("networkMedia");
    if (enabled) await api.runtime.sendMessage({ op: "enable_network_media" });
    setNotice(enabled ? "Reload the page and start playback, then scan again." : "Network discovery disabled; temporary offers cleared.");
  }
  async function floating(enabled) {
    if (enabled && !await api.permissions.request({ origins: ["http://*/*", "https://*/*"] })) { setError("Site permission is needed to show video buttons."); return; }
    await api.scripting.unregisterContentScripts({ ids: ["osdm-video-buttons"] }).catch(() => {});
    if (enabled) await api.scripting.registerContentScripts([{ id: "osdm-video-buttons", matches: ["http://*/*", "https://*/*"], js: ["video-button.js"], allFrames: true, runAt: "document_idle", persistAcrossSessions: true }]);
    await save({ floatingVideos: enabled }); setNotice(enabled ? "Video buttons enabled. Reload already-open pages; play a video to see its download button." : "Floating video buttons disabled.");
  }
  async function modifiers() {
    if (!await api.permissions.request({ origins: ["http://*/*", "https://*/*"] })) { setError("Site permission is needed for the Shift/Alt shortcut."); return; }
    await api.scripting.unregisterContentScripts({ ids: ["osdm-modifiers"] }).catch(() => {});
    await api.scripting.registerContentScripts([{ id: "osdm-modifiers", matches: ["http://*/*", "https://*/*"], js: ["modifiers.js"], runAt: "document_start", persistAcrossSessions: true }]);
    setNotice("Hold Shift or Alt while clicking to use the browser downloader. Reload existing tabs first.");
  }
  const title = { capture: "Your browser. Your engine.", media: "Keep the video.", rules: "Choose what comes through.", connection: "One local connection." }[route];
  return <div className="shell"><header><a className="brand" href="#/capture"><img src="icon-48.png" alt="" width="36" height="36" /><span>OSDM<small>CAPTURE DESK</small></span></a><button className="icon" aria-label={settings.theme === "light" ? "Use dark theme" : "Use light theme"} onClick={() => void act(() => save({ theme: settings.theme === "light" ? "dark" : "light" }))}>{settings.theme === "light" ? <Moon size={17} /> : <Sun size={17} />}</button></header>
    <nav aria-label="Extension pages">{routes.map(([path, label, Icon]) => <a key={path} href={`#/${path}`} aria-current={route === path ? "page" : undefined}><Icon size={16} strokeWidth={1.8} />{label}</a>)}</nav>
    <main aria-busy={busy}><div className="page-heading"><span className="eyebrow">{route === "capture" ? "BROWSER → LOCAL ENGINE" : route === "media" ? mediaSite(tab?.url) || "PAGE MEDIA" : route.toUpperCase()}</span><h1 ref={heading} tabIndex={-1}>{title}</h1></div>
      {!loaded ? <p role="status">Loading your settings…</p> : <>
      {route === "capture" && <>
        <section className="capture-state"><div className="signal" data-on={settings.enabled}><ArrowDownToLine size={26} /></div><div><h2>{settings.enabled ? "Automatic capture is on" : "Let OSDM take over"}</h2><p>{settings.enabled ? "Eligible files are handed to your local download queue." : "Switch it on to send matching browser downloads to OSDM."}</p></div></section>
        <section className="panel"><Switch label="Capture browser downloads" checked={settings.enabled} disabled={busy} onChange={(value) => void act(() => enableCapture(value))}>Saves immediately. OSDM must be running.</Switch><Switch label="Include login cookies" checked={settings.cookiesEnabled} disabled={busy} onChange={(value) => void act(() => shareCookies(value))}>On by default for every captured HTTPS download. Turn off to share no cookies automatically.</Switch></section>
        <div className="privacy"><ShieldCheck size={17} /><p>Only cookies matching the download URL leave the browser. They stay in the engine’s memory—not its database—and are never sent to other origins.</p></div>
        {settings.cookiesEnabled && cookieAccess === false && <div className="panel"><p className="hint">Allow cookie access once for automatic capture, or switch login cookie sharing off. No per-site approval is needed.</p><button className="primary" disabled={busy} onClick={() => void act(async () => { const allowed = await api.permissions.request(cookiePermission("all")); setCookieAccess(allowed); if (!allowed) setError("Cookie permission was declined."); })}>Allow automatic cookie access</button></div>}
        <a className="link-card" href="#/rules"><span><strong>{settings.extensions.split(/[\s,]+/).filter(Boolean).length} file types</strong><small>Programs, documents, archives, media and more</small></span><ArrowUpRight size={18} /></a>
        {lastCapture && <p className="hint">Last handoff: {({ captured: "sent to OSDM", disconnected: "app disconnected or disabled", cookie_unavailable: "cookies or cookie permission unavailable; kept in browser", prepare_failed: "app could not prepare the download", commit_failed: "check OSDM’s queue before retrying", handoff_failed: "handoff failed; check connection" })[lastCapture.outcome] || "check connection"}.</p>}
      </>}
      {route === "media" && <>
        <section className="panel"><Switch label="Media detection" checked={settings.mediaEnabled} disabled={busy} onChange={(value) => void act(async () => { await save({ mediaEnabled: value }); setOffers([]); await api.storage.session.remove("networkMedia"); })}>Manual review. Playback stays untouched.</Switch><Switch label="Floating video download button" checked={settings.floatingVideos} disabled={busy} onChange={(value) => void act(() => floating(value))}>A small button beside the video that is playing.</Switch><Switch label="Network media discovery" checked={settings.networkMediaEnabled} disabled={busy || !mediaAllowed(tab?.url, settings)} onChange={(value) => void act(() => network(value))}>Find HTTP sources behind blob players and embedded frames. Requires site permission.</Switch></section>
        <button className="primary wide" disabled={busy || !mediaAllowed(tab?.url, settings)} onClick={() => void act(scan)}><Film size={17} />Scan current page</button>
        <fieldset className="sources"><legend>Available sources</legend>{offers.length === 0 ? <p className="hint">Start playback, then scan. DRM and wholly local blobs aren’t supported.</p> : offers.map((offer, index) => <label className="source" key={offer.url}><input type="radio" name="source" checked={selected === index} onChange={() => { setSelected(index); setSubtitle(""); }} /><span><strong>{offer.kind}</strong><small>{new URL(offer.url).hostname} · {offer.title}</small></span></label>)}</fieldset>
        {selected != null && <><label className="field">Subtitle<select value={subtitle} onChange={(event) => setSubtitle(event.target.value)}><option value="">No subtitle</option>{offers[selected].subtitles.map((track, index) => <option key={track.url} value={index}>{track.label}</option>)}</select></label><button className="primary wide" disabled={busy} onClick={() => void act(async () => { const reply = await api.runtime.sendMessage({ op: "offer_media", index: selected, ...(subtitle !== "" ? { subtitleIndex: Number(subtitle) } : {}) }); if (!reply?.ok) { setError("Selection expired or OSDM is unavailable. Scan and try again."); return; } setOffers([]); setSelected(null); setNotice("Sent to OSDM. Review the source and quality there."); })}>Review download in OSDM<ArrowUpRight size={17} /></button></>}
        {mediaSite(tab?.url) && <Switch label="Disable detection on this site" checked={settings.mediaDisabledSites.includes(mediaSite(tab.url))} disabled={busy} onChange={(value) => void act(() => disableSite(value))}>{mediaSite(tab.url)}</Switch>}
      </>}
      {route === "rules" && <><label className="field">Captured file types<textarea rows={6} value={extensions} onChange={(event) => setExtensions(event.target.value)} /><small>Space- or comma-separated suffixes. Compound types such as tar.gz are supported.</small></label><label className="field">Excluded sites<textarea rows={3} placeholder="example.com, internal.example" value={exclusions} onChange={(event) => setExclusions(event.target.value)} /><small>Also excludes subdomains. These downloads stay with your browser.</small></label><div className="button-row"><button className="primary" disabled={busy} onClick={() => void act(async () => { await save({ extensions: extensions.slice(0, 16000), exclusions: exclusions.slice(0, 16000) }); setNotice("Capture rules saved."); })}><Check size={16} />Save rules</button><button disabled={busy} onClick={() => setExtensions(captureDefaults.extensions)}>Use built-in file types</button><button disabled={busy} onClick={() => void act(modifiers)}>Enable Shift/Alt bypass</button></div></>}
      {route === "connection" && <><section className="connection"><Cable size={30} /><h2>{connected === true ? "Connected to OSDM" : connected === false ? "OSDM is not ready" : "Check your connection"}</h2><p>Browser extension → native helper → authenticated localhost connection. No public port or cloud relay.</p><button className="primary" disabled={busy} onClick={() => void act(async () => { try { const reply = await api.runtime.sendNativeMessage("app.osdm.capture", { op: "ping" }); setConnected(reply?.ok === true && reply.version === 1); } catch { setConnected(false); } })}>Check connection</button></section><ol className="instructions"><li>Open OSDM and enable local browser integration.</li><li>Use Settings → Browser to register this extension’s ID for its browser.</li><li>Return here and check the connection.</li></ol><p className="hint">Missing helpers leave downloads in your browser. Private or partitioned cookie contexts that cannot be identified safely also stay in the browser.</p><button disabled={busy} onClick={() => void act(async () => { await api.action.setBadgeText({ text: "" }); setNotice("Warning badge cleared. This does not retry a download."); })}>Clear reviewed warning</button></>}
      </>}
      {error && <p role="alert" className="error">{error}</p>}{notice && <p role="status" className="notice">{notice}</p>}
    </main><footer><span>LOCAL BY DESIGN</span><span>OSDM / 0.1</span></footer></div>;
}
createRoot(document.getElementById("root")).render(api?.runtime?.id ? <App /> : <div className="shell"><main><h1>OSDM capture</h1><p>Open the installed browser extension to connect to OSDM. Browser preview cannot access cookies or downloads.</p></main></div>);
