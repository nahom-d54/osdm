import { collectMedia, isMediaPopup, mediaAllowed, mediaDefaults, normalizeMedia } from "./media.js";
import { installNetworkMedia } from "./network-media.js";

export function installMediaPanel(api, native) {
  const network = installNetworkMedia(api);
  let scan = null;
  const videoScans = new Map();
  async function scanPage(tab, settings, frameId) {
    let results = [];
    try { results = await api.scripting.executeScript({ target: frameId == null ? { tabId: tab.id, allFrames: true } : { tabId: tab.id, frameIds: [frameId] }, func: collectMedia }); }
    catch {
      if (frameId == null) {
        try { results = await api.scripting.executeScript({ target: { tabId: tab.id }, func: collectMedia }); } catch { /* Network-only discovery can still work. */ }
      }
    }
    const offers = []; const seen = new Set();
    const add = (candidate) => { if (offers.length < 32 && !seen.has(candidate.url)) { seen.add(candidate.url); offers.push(candidate); } };
    for (const frame of results.slice(0, 32)) for (const candidate of normalizeMedia(frame.result, frame.result?.base || tab.url)) add(candidate);
    for (const candidate of await network.offers(tab, settings)) add(candidate);
    return offers;
  }
  async function sendOffer(candidate, subtitle) {
    const reply = await native({ op: "offer_media", url: candidate.url, title: candidate.title,
      kind: candidate.kind === "HLS" ? "hls" : candidate.kind === "DASH" ? "dash" : "direct",
      ...(subtitle ? { subtitle_url: subtitle.url } : {}) });
    return { ok: reply?.ok === true, error: reply?.ok === true ? undefined : "unavailable" };
  }
  async function handleVideo(message, sender) {
    const settings = await api.storage.local.get(mediaDefaults);
    if (!settings.floatingVideos || !mediaAllowed(sender.url, settings) || !mediaAllowed(sender.tab.url, settings)) return { ok: false, error: "disabled" };
    const tab = await api.tabs.get(sender.tab.id);
    if (tab.url !== sender.tab.url) return { ok: false, error: "rescan" };
    const frame = new URL(sender.url);
    if (!await api.permissions.contains({ origins: [`${frame.protocol}//${frame.hostname}/*`] })) return { ok: false, error: "disabled" };
    if (message.op === "video_offers") {
      const offers = await scanPage(tab, settings, sender.frameId);
      const token = crypto.randomUUID();
      if (videoScans.size >= 64) videoScans.delete(videoScans.keys().next().value);
      videoScans.set(token, { tabId: tab.id, page: tab.url, frameId: sender.frameId, frameUrl: sender.url, offers, at: Date.now() });
      return { ok: true, token, offers };
    }
    const snapshot = videoScans.get(message.token);
    videoScans.delete(message.token);
    if (!snapshot || snapshot.tabId !== tab.id || snapshot.page !== tab.url || snapshot.frameId !== sender.frameId || snapshot.frameUrl !== sender.url || Date.now() - snapshot.at > 60000) return { ok: false, error: "rescan" };
    const candidate = Number.isInteger(message.index) && snapshot.offers[message.index];
    return candidate ? sendOffer(candidate) : { ok: false, error: "rescan" };
  }
  async function handle(message) {
    const [tab] = await api.tabs.query({ active: true, currentWindow: true });
    const settings = await api.storage.local.get(mediaDefaults);
    if (!tab?.id || !mediaAllowed(tab.url, settings)) {
      scan = null;
      return { ok: false, error: "disabled" };
    }
    if (message.op === "scan_media") {
      scan = null;
      const offers = await scanPage(tab, settings);
      scan = { tabId: tab.id, page: tab.url, offers, at: Date.now() };
      return { ok: true, offers };
    }
    if (message.op === "enable_network_media") { await network.activate(); return { ok: true }; }
    if (!scan || scan.tabId !== tab.id || scan.page !== tab.url || Date.now() - scan.at > 300000)
      return { ok: false, error: "rescan" };
    const candidate = Number.isInteger(message.index) && scan.offers[message.index];
    if (!candidate) return { ok: false, error: "rescan" };
    const subtitle = message.subtitleIndex == null ? null :
      Number.isInteger(message.subtitleIndex) && candidate.subtitles[message.subtitleIndex];
    if (message.subtitleIndex != null && !subtitle) return { ok: false, error: "rescan" };
    scan = null; // One explicit choice, never a bulk/playlist capture loop.
    return sendOffer(candidate, subtitle);
  }
  api.runtime.onMessage.addListener((message, sender, respond) => {
    if (["video_offers", "offer_video"].includes(message?.op)) {
      if (sender?.id !== api.runtime.id || !Number.isInteger(sender.tab?.id) || !Number.isInteger(sender.frameId)) return;
      void handleVideo(message, sender).then(respond, () => respond({ ok: false, error: "unavailable" }));
      return true;
    }
    if (!["scan_media", "offer_media", "enable_network_media"].includes(message?.op)) return;
    if (!isMediaPopup(sender, api.runtime)) return;
    void handle(message).then(respond, () => respond({ ok: false, error: "unavailable" }));
    return true;
  });
}
