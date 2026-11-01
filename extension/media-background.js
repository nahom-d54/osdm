import { collectMedia, isMediaPopup, mediaAllowed, mediaDefaults, normalizeMedia } from "./media.js";

export function installMediaPanel(api, native) {
  let scan = null;
  async function handle(message) {
    const [tab] = await api.tabs.query({ active: true, currentWindow: true });
    const settings = await api.storage.local.get(mediaDefaults);
    if (!tab?.id || !mediaAllowed(tab.url, settings)) {
      scan = null;
      return { ok: false, error: "disabled" };
    }
    if (message.op === "scan_media") {
      scan = null;
      const results = await api.scripting.executeScript({ target: { tabId: tab.id }, func: collectMedia });
      const offers = normalizeMedia(results[0]?.result, tab.url);
      scan = { tabId: tab.id, page: tab.url, offers, at: Date.now() };
      return { ok: true, offers };
    }
    if (!scan || scan.tabId !== tab.id || scan.page !== tab.url || Date.now() - scan.at > 300000)
      return { ok: false, error: "rescan" };
    const candidate = Number.isInteger(message.index) && scan.offers[message.index];
    if (!candidate) return { ok: false, error: "rescan" };
    const subtitle = message.subtitleIndex == null ? null :
      Number.isInteger(message.subtitleIndex) && candidate.subtitles[message.subtitleIndex];
    if (message.subtitleIndex != null && !subtitle) return { ok: false, error: "rescan" };
    scan = null; // One explicit choice, never a bulk/playlist capture loop.
    const reply = await native({ op: "offer_media", url: candidate.url, title: candidate.title,
      kind: candidate.kind === "HLS" ? "hls" : candidate.kind === "DASH" ? "dash" : "direct",
      ...(subtitle ? { subtitle_url: subtitle.url } : {}) });
    return { ok: reply?.ok === true, error: reply?.ok === true ? undefined : "unavailable" };
  }
  api.runtime.onMessage.addListener((message, sender, respond) => {
    if (!["scan_media", "offer_media"].includes(message?.op)) return;
    if (!isMediaPopup(sender, api.runtime)) return;
    void handle(message).then(respond, () => respond({ ok: false, error: "unavailable" }));
    return true;
  });
}
