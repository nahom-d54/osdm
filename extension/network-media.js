import { mediaAllowed, mediaUrl, normalizeMedia } from "./media.js";

const permission = { permissions: ["webRequest"], origins: ["http://*/*", "https://*/*"] };
const lifetime = 5 * 60 * 1000;

/** Bounded metadata observation, not response interception/decryption/page hooks. */
export function installNetworkMedia(api) {
  let listening = false;
  let work = Promise.resolve();
  let pending = 0;
  const generations = new Map();
  const requests = new Map();
  const frameEpochs = new Map();
  let epoch = 0;
  const frameKey = (tabId, frameId) => `${tabId}:${frameId}`;
  const newFrame = (key) => {
    frameEpochs.delete(key);
    frameEpochs.set(key, ++epoch);
    if (frameEpochs.size > 512) frameEpochs.delete(frameEpochs.keys().next().value);
    return epoch;
  };
  const before = (details) => {
    if (details.type && !["media", "xmlhttprequest", "other"].includes(details.type)) return;
    if (details.tabId < 0 || requests.has(details.requestId)) return;
    if (requests.size >= 512) requests.delete(requests.keys().next().value);
    const frameId = Number.isInteger(details.frameId) ? details.frameId : -1;
    const key = frameKey(details.tabId, frameId);
    requests.set(details.requestId, { tabId: details.tabId, generation: generations.get(details.tabId) || 0,
      frameId, documentId: details.documentId || null, frameUrl: mediaUrl(details.documentUrl), frameOrigin: mediaUrl(details.initiator), key,
      epoch: frameEpochs.get(key) || newFrame(key) });
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
    if (!request || request.tabId !== details.tabId || request.generation !== (generations.get(details.tabId) || 0) || frameEpochs.get(request.key) !== request.epoch) return;
    if (details.tabId < 0 || !((details.statusCode >= 200 && details.statusCode < 300) || details.statusCode === 304)) return;
    const url = mediaUrl(details.url);
    if (!url || /\.(?:ts|m4s|cmfv|cmfa|cmft|vtt|srt|key)(?:[?#]|$)/i.test(url)) return;
    const type = details.responseHeaders?.slice(0, 64).find((header) => header.name.toLowerCase() === "content-type")?.value?.slice(0, 128) || "";
    if (/^\s*(?:video\/mp2t|(?:video|audio|application)\/iso\.segment)(?:\s*;|\s*$)/i.test(type)) return;
    if (!normalizeMedia({ entries: [{ url, type, source: "network" }] }, url).length) return;
    return enqueue(async () => {
      const settings = await api.storage.local.get({ mediaEnabled: true, mediaDisabledSites: [], networkMediaEnabled: true });
      if (!settings.networkMediaEnabled || !await api.permissions.contains(permission)) return;
      const tab = await api.tabs.get(details.tabId);
      if (!mediaAllowed(tab.url, settings) || ((request.frameUrl || request.frameOrigin) && !mediaAllowed(request.frameUrl || request.frameOrigin, settings))) return;
      const now = Date.now();
      const cache = await read();
      if (request.generation !== (generations.get(details.tabId) || 0) || frameEpochs.get(request.key) !== request.epoch) return;
      for (const [id, entry] of Object.entries(cache)) if (now - entry.at > lifetime) delete cache[id];
      const key = String(tab.id);
      if (cache[key]?.page !== tab.url) delete cache[key];
      const entry = cache[key] || { page: tab.url, entries: [], at: now };
      entry.entries = entry.entries.filter((candidate) => now - (candidate.at || entry.at) <= lifetime && (candidate.url !== url || candidate.frameId !== request.frameId || candidate.documentId !== request.documentId));
      entry.entries.push({ url, type, source: "network", frameId: request.frameId, documentId: request.documentId, frameUrl: request.frameUrl, frameOrigin: request.frameOrigin, at: now });
      entry.entries = entry.entries.slice(-32);
      entry.at = now;
      cache[key] = entry;
      while (Object.keys(cache).length > 8) delete cache[Object.keys(cache).sort((a, b) => cache[a].at - cache[b].at)[0]];
      await save(cache);
    });
  };
  function activate() {
    if (listening || !api.webRequest?.onHeadersReceived) return;
    api.webRequest.onHeadersReceived.addListener(receive, { urls: permission.origins, types: ["media", "xmlhttprequest", "other"] }, ["responseHeaders"]);
    api.webRequest.onBeforeRequest.addListener(before, { urls: permission.origins, types: ["media", "xmlhttprequest", "other"] });
    api.webRequest.onBeforeRequest.addListener(navigation, { urls: permission.origins, types: ["main_frame", "sub_frame"] });
    api.webRequest.onCompleted.addListener(complete, { urls: permission.origins });
    api.webRequest.onErrorOccurred.addListener(complete, { urls: permission.origins });
    listening = true;
  }
  const forget = (tabId) => {
    generations.set(tabId, (generations.get(tabId) || 0) + 1);
    for (const [id, request] of requests) if (request.tabId === tabId) requests.delete(id);
    return enqueue(async () => { const cache = await read(); delete cache[String(tabId)]; await save(cache); });
  };
  const navigation = (details) => {
    if (details.tabId < 0 || !["main_frame", "sub_frame"].includes(details.type)) return;
    if (details.type === "main_frame") return forget(details.tabId);
    newFrame(frameKey(details.tabId, details.frameId));
    return enqueue(async () => {
      const cache = await read();
      if (cache[String(details.tabId)]) cache[String(details.tabId)].entries = cache[String(details.tabId)].entries.filter((entry) => entry.frameId !== details.frameId);
      await save(cache);
    });
  };
  api.tabs?.onRemoved?.addListener((tabId) => { const result = forget(tabId); generations.delete(tabId); for (const key of frameEpochs.keys()) if (key.startsWith(`${tabId}:`)) frameEpochs.delete(key); return result; });
  api.tabs?.onUpdated?.addListener((tabId, change) => { if (change.url || change.status === "loading") return forget(tabId); });
  api.permissions?.onAdded?.addListener(() => { try { activate(); } catch { /* Browser permission still unavailable. */ } });
  api.permissions?.onRemoved?.addListener(async () => {
    if (await api.permissions.contains(permission)) return;
    if (listening) {
      api.webRequest.onHeadersReceived.removeListener(receive);
      api.webRequest.onBeforeRequest.removeListener(before);
      api.webRequest.onBeforeRequest.removeListener(navigation);
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
  // Required permissions are declared in the manifest. Register synchronously
  // so an MV3 worker wake-up does not miss its first request. Reads still check
  // current permission and settings, including after a revocation.
  try { activate(); } catch { /* The browser may withhold site permissions. */ }
  return {
    activate,
    async offers(tab, settings, context, blockedFrames) {
      await work;
      if (!settings.networkMediaEnabled || !await api.permissions.contains(permission)) return [];
      const cache = await read();
      const entry = cache[String(tab.id)];
      if (entry && Date.now() - entry.at > lifetime) { delete cache[String(tab.id)]; await save(cache); return []; }
      const entries = (entry?.entries || []).filter((candidate) => {
        if (Date.now() - (candidate.at || entry.at) > lifetime) return false;
        if (blockedFrames?.has(candidate.frameId)) return false;
        if ((candidate.frameUrl || candidate.frameOrigin) && !mediaAllowed(candidate.frameUrl || candidate.frameOrigin, settings)) return false;
        if (!context && candidate.frameId !== 0 && settings.mediaDisabledSites?.length && !candidate.frameUrl && !candidate.frameOrigin) return false;
        if (!context) return true;
        if (candidate.frameId !== context.frameId) return false;
        if (context.documentId && candidate.documentId !== context.documentId) return false;
        return !candidate.frameUrl || candidate.frameUrl === mediaUrl(context.url);
      });
      return entry?.page === tab.url ? normalizeMedia({ title: tab.title, entries }, tab.url) : [];
    },
  };
}
