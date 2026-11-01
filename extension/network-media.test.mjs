import test from "node:test";
import assert from "node:assert/strict";
import { installNetworkMedia } from "./network-media.js";

function event() {
  const listeners = new Set();
  return { addListener(fn) { listeners.add(fn); }, removeListener(fn) { listeners.delete(fn); }, async fire(...args) { for (const listener of listeners) await listener(...args); } };
}
async function harness() {
  let granted = true;
  let tab = { id: 7, url: "https://page.example/watch", title: "Blob player" };
  const state = {};
  const settings = { mediaEnabled: true, mediaDisabledSites: [], networkMediaEnabled: true };
  const api = {
    permissions: { contains: async () => granted, onAdded: event(), onRemoved: event() },
    webRequest: { onBeforeRequest: event(), onHeadersReceived: event(), onCompleted: event(), onErrorOccurred: event() },
    tabs: { get: async () => tab, onRemoved: event(), onUpdated: event() },
    storage: { local: { get: async () => settings }, session: { get: async (defaults) => structuredClone({ ...defaults, ...state }), set: async (value) => Object.assign(state, structuredClone(value)) } },
  };
  const discovery = installNetworkMedia(api);
  await discovery.activate();
  let id = 0;
  const start = async (url, context = {}) => { const details = { requestId: String(++id), tabId: 7, frameId: 3, url, statusCode: 200, ...context }; await api.webRequest.onBeforeRequest.fire(details); return details; };
  return { api, settings, state, start, offers: (context) => discovery.offers(tab, settings, context), async record(url, type, context) {
    const details = await start(url, context);
    await api.webRequest.onHeadersReceived.fire({ ...details, responseHeaders: [{ name: "Content-Type", value: type }] });
    await api.webRequest.onCompleted.fire(details);
  }, async navigate() { tab = { ...tab, url: "https://page.example/next" }; await api.tabs.onUpdated.fire(7, { url: tab.url }); }, async revoke() { granted = false; await api.permissions.onRemoved.fire(); } };
}

test("opt-in response metadata discovers an extensionless manifest requested by a blob/JS player inside a frame", async () => {
  const h = await harness();
  await h.record("https://cdn.example/stream?token=transient", "application/vnd.apple.mpegurl");
  assert.deepEqual((await h.offers()).map((offer) => [offer.url, offer.kind]), [["https://cdn.example/stream?token=transient", "HLS"]]);
  assert.equal(JSON.stringify(h.state).includes("Cookie"), false);
  await h.navigate();
  assert.deepEqual(await h.offers(), []);
});
test("network discovery rejects credentials, blob URLs, raw segments and unrelated JavaScript requests", async () => {
  const h = await harness();
  for (const [url, type] of [["https://user:password@cdn.example/a.m3u8", "application/vnd.apple.mpegurl"], ["blob:https://page.example/id", "video/mp4"], ["https://cdn.example/seg.ts", "video/mp2t"], ["https://cdn.example/seg.m4s", "video/mp4"], ["https://cdn.example/api", "application/json"]]) await h.record(url, type);
  assert.deepEqual(await h.offers(), []);
});
test("network observation respects global/site disable and permission revocation", async () => {
  const h = await harness();
  h.settings.mediaDisabledSites = ["page.example"];
  await h.record("https://cdn.example/blocked.mpd", "application/dash+xml");
  assert.deepEqual(await h.offers(), []);
  h.settings.mediaDisabledSites = []; h.settings.mediaEnabled = false;
  await h.record("https://cdn.example/off.mpd", "application/dash+xml");
  h.settings.mediaEnabled = true;
  await h.record("https://cdn.example/allowed.mpd", "application/dash+xml");
  assert.equal((await h.offers()).length, 1);
  await h.revoke();
  assert.deepEqual(await h.offers(), []);
});
test("late responses from an old document cannot become offers for the next page", async () => {
  const h = await harness();
  const old = await h.start("https://cdn.example/old.mpd");
  await h.navigate();
  await h.api.webRequest.onHeadersReceived.fire({ ...old, responseHeaders: [{ name: "Content-Type", value: "application/dash+xml" }] });
  assert.deepEqual(await h.offers(), []);
});
test("observed candidates stay bounded and expire before a later scan", async () => {
  const h = await harness();
  for (let index = 0; index < 70; index++) await h.record(`https://cdn.example/${index}.mp4`, "video/mp4");
  assert.equal((await h.offers()).length, 32);
  h.state.networkMedia["7"].at = Date.now() - 300001;
  assert.deepEqual(await h.offers(), []);
});

test("floating discovery isolates sibling frames and same-URL document replacements", async () => {
  const h = await harness();
  const first = { frameId: 3, documentId: "player-document", documentUrl: "https://player.example/embed" };
  const second = { frameId: 4, documentId: "ad-document", documentUrl: "https://ad.example/embed" };
  await h.record("https://cdn.example/film.m3u8", "application/x-mpegURL", first);
  await h.record("https://cdn.example/ad.m3u8", "application/x-mpegURL", second);
  assert.equal((await h.offers()).length, 2);
  const context = { frameId: 3, documentId: "player-document", url: first.documentUrl };
  assert.deepEqual((await h.offers(context)).map((offer) => offer.url), ["https://cdn.example/film.m3u8"]);
  assert.deepEqual(await h.offers({ ...context, documentId: "new-document" }), []);
  h.settings.mediaDisabledSites = ["player.example"];
  assert.deepEqual(await h.offers(context), []);
});

test("iframe navigation invalidates only its offers and rejects late responses", async () => {
  const h = await harness();
  await h.record("https://cdn.example/old.mpd", "application/dash+xml");
  await h.record("https://cdn.example/other.mpd", "application/dash+xml", { frameId: 4 });
  const late = await h.start("https://cdn.example/late.mpd");
  await h.api.webRequest.onBeforeRequest.fire({ requestId: "nav", tabId: 7, frameId: 3, type: "sub_frame", url: "https://player.example/new" });
  await h.api.webRequest.onHeadersReceived.fire({ ...late, responseHeaders: [{ name: "Content-Type", value: "application/dash+xml" }] });
  assert.deepEqual((await h.offers()).map((offer) => offer.url), ["https://cdn.example/other.mpd"]);
});

test("cached manifests and uncommon direct containers are found, not raw extensionless segments", async () => {
  const h = await harness();
  await h.record("https://cdn.example/cached.m3u8", "", { statusCode: 304 });
  await h.record("https://cdn.example/file", " Video/MP4; codecs=avc1 ");
  await h.record("https://cdn.example/legacy.avi", "application/octet-stream");
  await h.record("https://cdn.example/segment", "video/mp2t");
  await h.record("https://cdn.example/fragment", "video/iso.segment");
  assert.deepEqual((await h.offers()).map((offer) => offer.kind), ["HLS", "Direct media", "Direct media"]);
});

test("busy pages do not extend older source lifetimes", async () => {
  const h = await harness();
  await h.record("https://cdn.example/old.mp4", "video/mp4");
  h.state.networkMedia["7"].entries[0].at = Date.now() - 300001;
  await h.record("https://cdn.example/current.mp4", "video/mp4");
  assert.deepEqual((await h.offers()).map((offer) => offer.url), ["https://cdn.example/current.mp4"]);
});

test("MV3 request listeners are registered before the first asynchronous permission check", () => {
  const headers = event(), before = event();
  let checks = 0;
  const registered = [];
  installNetworkMedia({ permissions: { contains: async () => { checks++; return true; } },
    webRequest: { onHeadersReceived: { ...headers, addListener: (...args) => { registered.push("headers"); headers.addListener(...args); } }, onBeforeRequest: { ...before, addListener: (...args) => { registered.push("before"); before.addListener(...args); } }, onCompleted: event(), onErrorOccurred: event() } });
  assert.deepEqual(registered, ["headers", "before", "before"]);
  assert.equal(checks, 0);
});

test("Chrome initiator origins enforce disabled embedded sites when documentUrl is absent", async () => {
  const h = await harness();
  h.settings.mediaDisabledSites = ["disabled.example"];
  await h.record("https://cdn.example/blocked.mpd", "application/dash+xml", { initiator: "https://disabled.example" });
  await h.record("https://cdn.example/allowed.mpd", "application/dash+xml", { initiator: "https://allowed.example" });
  assert.deepEqual((await h.offers()).map((offer) => offer.url), ["https://cdn.example/allowed.mpd"]);
});
