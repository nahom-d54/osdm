import { useEffect, useId, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Field, ErrorNote } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { getBridge } from "@/lib/bridge";
import { BROWSER_MANUAL_SETUP, validExtensionId, type BrowserSetupStatus, type BrowserTarget } from "@/lib/browser-setup";

/** A user-driven wiring checklist, not a claimed extension-install detector. */
export function BrowserSetup({ onCaptureChanged }: { onCaptureChanged?: (enabled: boolean) => void }) {
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState<BrowserSetupStatus | null>(null);
  const [browser, setBrowser] = useState<BrowserTarget>("chrome");
  const [extensionId, setExtensionId] = useState("");
  const [consent, setConsent] = useState(false);
  const [removeConsent, setRemoveConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const pending = useRef(false);
  const opener = useRef<HTMLButtonElement>(null);
  const first = useRef<HTMLSelectElement>(null);
  const generation = useRef(0);
  const id = useId();
  const selected = status?.browsers.find((value) => value.browser === browser);
  const actualId = selected?.extension_id ?? extensionId;
  const refresh = async () => { const next = await getBridge().browserSetupStatus(); setStatus(next); return next; };
  useEffect(() => {
    generation.current += 1;
    if (!open) return;
    const current = generation.current;
    first.current?.focus();
    setError(null);
    let loading = false;
    const load = async () => {
      if (loading || pending.current) return;
      loading = true;
      try { const next = await getBridge().browserSetupStatus(); if (current === generation.current) setStatus(next); }
      catch (error) { if (current === generation.current) setError(String(error)); }
      finally { loading = false; }
    };
    void load();
    const timer = setInterval(() => void load(), 3000);
    return () => { generation.current += 1; clearInterval(timer); };
  }, [open]);
  const act = async (operation: () => Promise<void>) => {
    if (pending.current) return;
    pending.current = true; setBusy(true); setError(null); setNotice("");
    try { await operation(); }
    catch (error) { setError(String(error)); }
    finally { pending.current = false; setBusy(false); }
  };
  return <section className="space-y-3" aria-label="Guided browser integration">
    <Button ref={opener} size="sm" variant="outline" aria-expanded={open} aria-controls={id} disabled={busy} onClick={() => { setOpen(!open); setConsent(false); setRemoveConsent(false); if (open) opener.current?.focus(); }}>Set up browser extension</Button>
    {open && <div id={id} className="space-y-4 rounded-md border border-brand-line bg-brand-soft p-4">
      <div><h3 className="text-sm font-medium">Connect your browser to OSDM</h3><p className="mt-1 text-xs text-muted-foreground">Your browser approves extension installation. OSDM registers only its packaged helper for your Windows account—no administrator permission, browser-profile scanning, or silent installation.</p></div>
      <Field label="Browser to connect" htmlFor={`${id}-browser`}><select ref={first} id={`${id}-browser`} className="h-9 w-full rounded-md border border-line bg-panel px-3 text-sm" disabled={busy} value={browser} onChange={(event) => { setBrowser(event.target.value as BrowserTarget); setExtensionId(""); setConsent(false); setRemoveConsent(false); setNotice(""); setError(null); }}><option value="chrome">Google Chrome</option><option value="edge">Microsoft Edge</option><option value="firefox">Mozilla Firefox</option></select></Field>
      <ol className="space-y-4">
        <li className="space-y-2 border-t border-line pt-3"><h4 className="text-sm font-medium">1 · Add the extension</h4>
          {selected?.store_url ? <><p className="text-xs text-muted-foreground">Open the configured store listing, approve the browser's installation prompt, then return here.</p><Button size="sm" disabled={busy} onClick={() => void act(async () => { await getBridge().openExtensionStore(browser); setNotice("Store listing opened. Approve installation in your browser, then continue here."); })}>Open extension store</Button></> : <p className="text-xs text-muted-foreground">No published store listing is configured for this browser in this build. Use the packaged extension files for manual setup; OSDM will not send you to a placeholder listing.</p>}
          <p className="text-xs text-muted-foreground">{BROWSER_MANUAL_SETUP[browser]}</p>
          <Button size="sm" variant="outline" disabled={busy || !selected?.unpacked_available} onClick={() => void act(async () => { await getBridge().openExtensionFiles(browser); setNotice("Extension files opened. Follow the browser's manual loading steps above."); })}>Open extension files</Button>
          {status && !selected?.unpacked_available && <p className="text-xs text-muted-foreground">Extension files are missing. Install a release built with browser resources, or follow extension/README.md from the source checkout.</p>}
        </li>
        <li className="space-y-2 border-t border-line pt-3"><h4 className="text-sm font-medium">2 · Register the local helper</h4>
          <p className="text-xs text-muted-foreground">{selected?.registered ? "OSDM's helper is registered for this browser." : "Register the installed extension's exact ID. This changes only this browser's per-user native-host registration."}</p>
          {status && !status.helper_available && <ErrorNote>This build has no packaged native helper. Install a complete release bundle or build it with pnpm desktop:build, then reopen setup.</ErrorNote>}
          {status && !status.registration_supported && <p className="text-xs text-muted-foreground">Guided registration currently requires Windows. Other platforms use the manual instructions in extension/README.md.</p>}
          {selected?.conflict && <ErrorNote>A different native-host registration already exists. OSDM will not overwrite it. Remove or migrate that registration manually, then refresh.</ErrorNote>}
          <Field label="Installed extension ID" htmlFor={`${id}-extension`} hint={<span id={`${id}-identity-hint`} className="text-muted-foreground">{browser === "firefox" ? "Firefox uses the fixed OSDM extension identity." : "Copy the 32-letter a–p ID from your browser's extensions page. A configured store ID cannot be changed here."}</span>}><Input id={`${id}-extension`} aria-describedby={`${id}-identity-hint`} value={actualId} maxLength={64} readOnly={!!selected?.extension_id} disabled={busy} onChange={(event) => { setExtensionId(event.target.value.trim()); setConsent(false); }} autoComplete="off" spellCheck={false} /></Field>
          <label className="flex items-start gap-2 text-xs"><input type="checkbox" disabled={busy} checked={consent} onChange={(event) => setConsent(event.target.checked)} /><span>I authorize OSDM to register its packaged helper for this browser and Windows account.</span></label>
          <Button size="sm" disabled={busy || !consent || !status?.registration_supported || !status.helper_available || selected?.conflict || !validExtensionId(browser, actualId)} onClick={() => void act(async () => { await getBridge().registerBrowserHelper(browser, actualId, true); setConsent(false); await refresh(); setNotice("Browser helper registered. Restart the browser if it was already open, then check the connection."); })}>Register browser helper</Button>
          {selected?.registered && <div className="space-y-2 pt-2"><label className="flex items-start gap-2 text-xs"><input type="checkbox" checked={removeConsent} disabled={busy} onChange={(event) => setRemoveConsent(event.target.checked)} /><span>I authorize removing only OSDM's helper registration for this browser.</span></label><Button size="sm" variant="ghost" disabled={busy || !removeConsent} onClick={() => void act(async () => { await getBridge().unregisterBrowserHelper(browser, true); setRemoveConsent(false); await refresh(); setNotice("Browser helper registration removed. The extension, files and other browsers are unchanged."); })}>Remove helper registration</Button></div>}
        </li>
        <li className="space-y-2 border-t border-line pt-3"><h4 className="text-sm font-medium">3 · Enable and check the connection</h4>
          <p className="text-xs text-muted-foreground">Enable local integration, then open the extension popup and click Check connection. Automatic download interception remains a separate, opt-in extension setting.</p>
          <Button size="sm" variant="outline" disabled={busy || !selected?.registered || !status?.helper_available || !!status?.capture_enabled} onClick={() => void act(async () => { await getBridge().enableCapture(true); onCaptureChanged?.(true); await refresh(); setNotice("Local integration enabled. Click Check connection in the browser extension popup."); })}>{status?.capture_enabled ? "Local integration enabled" : "Enable local integration"}</Button>
          <p role="status" className="text-xs">{status?.last_handshake ? `Last successful native-helper check this session: ${new Date(status.last_handshake * 1000).toLocaleTimeString()}.` : "No successful native-helper check received this session yet."}</p>
          <p className="text-xs text-muted-foreground">The check is session-wide, not proof that a particular browser or extension is installed. Setup never inspects your browser profiles.</p>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => void act(async () => { await refresh(); setNotice("Browser setup status refreshed."); })}>Refresh setup status</Button>
        </li>
      </ol>
      {notice && <p role="status" className="text-xs text-ok">{notice}</p>}{error && <ErrorNote>{error}</ErrorNote>}
    </div>}
  </section>;
}
