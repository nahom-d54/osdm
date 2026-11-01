import { mediaAllowed, mediaUrl, normalizeMedia } from "./media.js";

const permission = { permissions: ["webRequest"], origins: ["http://*/*", "https://*/*"] };
const lifetime = 5 * 60 * 1000;

/** Opt-in metadata observation, not response interception/decryption/page hooks. */
export function installNetworkMedia(api) {
  let listening = false;
  let work = Promise.resolve();
  let pending = 0;
  const generations = new Map();
  const requests = new Map();
  const before = (details) => {
    if (details.tabId < 0 || requests.has(details.requestId)) return;
    if (requests.size >= 512) requests.delete(requests.keys().next().value);
    requests.set(details.requestId, { tabId: details.tabId, generation: generations.get(details.tabId) || 0 });
  };
  const complete = (details) => requests.delete(details.requestId);
  const enqueue = (operation) => {
    if (pending >= 32) return Promise.resolve();
    pending++;
    work = work.then(operation).catch(() => {}).finally(() => { pending--; });
    return work;
  };
  const read = async () => (await api.storage.session.get({ networkMedia: {} })).networkMedia;
  const save = (networkMedia) => api.storage.session.set({ networkMedia });
  const receive = (details) => {
    const request = requests.get(details.requestId);
    if (!request || request.tabId !== details.tabId || request.generation !== (generations.get(details.tabId) || 0)) return;
    if (details.tabId < 0 || details.statusCode < 200 || details.statusCode >= 300) return;
    const url = mediaUrl(details.url);
    if (!url || /\.(?:ts|m4s|cmfv|cmfa|vtt|srt|key)(?:[?#]|$)/i.test(url)) return;
    const type = details.responseHeaders?.slice(0, 64).find((header) => header.name.toLowerCase() === "content-type")?.value?.slice(0, 128) || "";
    if (!normalizeMedia({ entries: [{ url, type, source: "network" }] }, url).length) return;
    return enqueue(async () => {
      const settings = await api.storage.local.get({ mediaEnabled: true, mediaDisabledSites: [], networkMediaEnabled: false });
      if (!settings.networkMediaEnabled) return;
      const tab = await api.tabs.get(details.tabId);
      if (!mediaAllowed(tab.url, settings)) return;
      const now = Date.now();
      const cache = await read();
      if (request.generation !== (generations.get(details.tabId) || 0)) return;
      for (const [id, entry] of Object.entries(cache)) if (now - entry.at > lifetime) delete cache[id];
      const key = String(tab.id);
      if (cache[key]?.page !== tab.url) delete cache[key];
      const entry = cache[key] || { page: tab.url, entries: [], at: now };
      entry.entries = entry.entries.filter((entry) => entry.url !== url);
      entry.entries.push({ url, type, source: "network" });
      entry.entries = entry.entries.slice(-32);
      entry.at = now;
      cache[key] = entry;
      while (Object.keys(cache).length > 8) delete cache[Object.keys(cache).sort((a, b) => cache[a].at - cache[b].at)[0]];
      await save(cache);
    });
  };
  async function activate() {
    if (listening || !api.webRequest?.onHeadersReceived || !await api.permissions.contains(permission) || listening) return;
    api.webRequest.onHeadersReceived.addListener(receive, { urls: permission.origins, types: ["media", "xmlhttprequest", "other"] }, ["responseHeaders"]);
    api.webRequest.onBeforeRequest.addListener(before, { urls: permission.origins, types: ["media", "xmlhttprequest", "other"] });
    api.webRequest.onCompleted.addListener(complete, { urls: permission.origins });
    api.webRequest.onErrorOccurred.addListener(complete, { urls: permission.origins });
    listening = true;
  }
  const forget = (tabId) => {
    generations.set(tabId, (generations.get(tabId) || 0) + 1);
    for (const [id, request] of requests) if (request.tabId === tabId) requests.delete(id);
    return enqueue(async () => { const cache = await read(); delete cache[String(tabId)]; await save(cache); });
  };
  api.tabs?.onRemoved?.addListener((tabId) => { const result = forget(tabId); generations.delete(tabId); return result; });
  api.tabs?.onUpdated?.addListener((tabId, change) => { if (change.url || change.status === "loading") return forget(tabId); });
  api.permissions?.onAdded?.addListener(() => { void activate().catch(() => {}); });
  api.permissions?.onRemoved?.addListener(async () => {
    if (await api.permissions.contains(permission)) return;
    if (listening) {
      api.webRequest.onHeadersReceived.removeListener(receive);
      api.webRequest.onBeforeRequest.removeListener(before);
      api.webRequest.onCompleted.removeListener(complete);
      api.webRequest.onErrorOccurred.removeListener(complete);
    }
    requests.clear();
    listening = false;
    return enqueue(() => save({}));
  });
  api.storage?.onChanged?.addListener((changes, area) => {
    if (area === "local" && (changes.mediaEnabled || changes.mediaDisabledSites || changes.networkMediaEnabled)) return enqueue(() => save({}));
  });
  void activate().catch(() => {});
  return {
    activate,
    async offers(tab, settings) {
      await work;
      if (!settings.networkMediaEnabled || !await api.permissions.contains(permission)) return [];
      const cache = await read();
      const entry = cache[String(tab.id)];
      if (entry && Date.now() - entry.at > lifetime) { delete cache[String(tab.id)]; await save(cache); return []; }
      return entry?.page === tab.url ? normalizeMedia({ title: tab.title, entries: entry.entries }, tab.url) : [];
    },
  };
}
