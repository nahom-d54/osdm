import test from "node:test";
import assert from "node:assert/strict";
import { collectMedia, isMediaPopup, mediaAllowed, mediaDefaults, mediaLimits, mediaTitle, mediaUrl, normalizeMedia, toggleMediaSite } from "./media.js";
import { installMediaPanel } from "./media-background.js";

test("media URL normalization rejects credentials, non-HTTP sources, malformed and oversized URLs", () => {
  assert.equal(mediaUrl("../film.mp4?token=transient#part", "https://media.example/player/"), "https://media.example/film.mp4?token=transient");
  for (const url of ["blob:https://media.example/id", "data:video/mp4,abc", "javascript:alert(1)", "file:///video.mp4", "https://user:pass@media.example/a.mp4", "http://[", "https://media.example/" + "x".repeat(mediaLimits.url)])
    assert.equal(mediaUrl(url), null);
  assert.equal(mediaTitle('A / page: title?\n'), "A page title");
  assert.equal(mediaTitle("."), "Page media");
  assert.equal(mediaTitle("x".repeat(200)).length, 160);
});

test("discovery uses real DOM/resource entries, deduplicates, classifies manifests and associates only player tracks", () => {
  const offers = normalizeMedia({ title: "Real player", entries: [
    { url: "/watch", source: "video", group: 0 },
    { url: "/watch#fragment", source: "source", group: 0 },
    { url: "/en.vtt", source: "track", group: 0, label: "English" },
    { url: "/other.vtt", source: "track", group: 1 },
    { url: "/master.m3u8?signature=ephemeral", source: "resource" },
    { url: "/stream.mpd", source: "resource" },
    { url: "/sound.mp3", source: "audio" },
    { url: "/unknown", source: "resource" },
    { url: "/segment.ts", source: "resource" },
    { url: "blob:https://page.example/id", source: "video" },
  ] }, "https://page.example/player");
  assert.equal(offers.length, 4);
  assert.deepEqual(offers.map((o) => o.kind), ["Direct media", "HLS", "DASH", "Direct media"]);
  assert.deepEqual(offers[0].subtitles, [{ url: "https://page.example/en.vtt", label: "English" }]);
  assert.deepEqual(offers[1].subtitles, []);
  assert.equal(offers[1].url, "https://page.example/master.m3u8?signature=ephemeral");
  assert.deepEqual(normalizeMedia({}, "https://page.example"), []);
});

test("discovery bounds both inspected entries and output offers", () => {
  const entries = Array.from({ length: 1000 }, (_, i) => ({ url: `https://media.example/${i}.mp4`, source: "resource" }));
  assert.equal(normalizeMedia({ entries }, "https://page.example").length, mediaLimits.offers);
  const ignored = Array.from({ length: mediaLimits.entries }, () => ({ url: "blob:ignored", source: "video" }));
  assert.deepEqual(normalizeMedia({ entries: [...ignored, entries[0]] }, "https://page.example"), []);
});

test("injected collector reads media/source/track and only recent media resources without controlling playback", () => {
  const video = { localName: "video", currentSrc: "https://media.example/live.m3u8", src: "", children: [
    { localName: "source", src: "https://media.example/backup.mp4", type: "video/mp4" },
    { localName: "track", src: "https://media.example/en.vtt", kind: "subtitles", label: "English" },
  ] };
  const drm = { localName: "video", currentSrc: "https://media.example/protected.mpd", mediaKeys: {}, children: [] };
  const audio = { localName: "audio", currentSrc: "https://media.example/audio.mp3", src: "", children: [] };
  const nodes = [{ localName: "html" }, video, drm, audio];
  const originalDocument = globalThis.document;
  const originalPerformance = globalThis.performance;
  globalThis.document = {
    documentElement: nodes[0], title: "Actual page title",
    createTreeWalker: () => { let index = 0; return { currentNode: nodes[0], nextNode: () => nodes[++index] }; },
  };
  globalThis.performance = { getEntriesByType: () => [
    { name: "https://media.example/old.mpd" },
    ...Array.from({ length: 255 }, () => ({ name: "https://media.example/segment.ts" })),
    { name: "https://media.example/new.mpd?transient=token" },
  ] };
  try {
    const snapshot = collectMedia();
    assert.equal(snapshot.title, "Actual page title");
    const urls = normalizeMedia(snapshot, "https://page.example").map((o) => o.url);
    assert.deepEqual(urls, ["https://media.example/live.m3u8", "https://media.example/backup.mp4", "https://media.example/audio.mp3", "https://media.example/new.mpd?transient=token"]);
    assert.equal(snapshot.entries.find((entry) => entry.source === "track").group, 0);
  } finally {
    globalThis.document = originalDocument;
    globalThis.performance = originalPerformance;
  }
});

test("manual detection defaults on but global and exact-site disable persist without media URLs", () => {
  assert.equal(mediaAllowed("https://page.example/watch?secret=transient", mediaDefaults), true);
  const disabled = toggleMediaSite([], "https://page.example/watch?secret=transient", true);
  assert.deepEqual(disabled, ["page.example"]);
  assert.equal(mediaAllowed("https://page.example/other", { ...mediaDefaults, mediaDisabledSites: disabled }), false);
  assert.equal(mediaAllowed("https://sub.page.example/", { ...mediaDefaults, mediaDisabledSites: disabled }), true);
  assert.equal(mediaAllowed("https://page.example", { ...mediaDefaults, mediaEnabled: false }), false);
  assert.equal(mediaAllowed("chrome://extensions", mediaDefaults), false);
  assert.deepEqual(toggleMediaSite(disabled, "https://page.example/", false), []);
  assert.deepEqual(toggleMediaSite(disabled, "blob:invalid", true), disabled);
});

test("only the exact extension UI is trusted; tab views also require the extension origin", () => {
  const runtime = { id: "extension-id", getURL: (file) => `chrome-extension://extension-id/${file}` };
  const sender = { id: runtime.id, url: runtime.getURL("options.html") };
  assert.equal(isMediaPopup(sender, runtime), true);
  assert.equal(isMediaPopup({ ...sender, url: sender.url + "#/media" }, runtime), true);
  assert.equal(isMediaPopup({ ...sender, tab: { id: 1 }, origin: "chrome-extension://extension-id" }, runtime), true);
  for (const untrusted of [{ ...sender, tab: { id: 1 } }, { ...sender, url: "https://page.example/" }, { ...sender, id: "another-extension" }, { ...sender, url: sender.url + "?spoof" }])
    assert.equal(isMediaPopup(untrusted, runtime), false);
});

function harness({ reply = { ok: true }, settings = mediaDefaults, frames, denyAllFrames = false } = {}) {
  let listener;
  let tab = { id: 7, url: "https://page.example/player" };
  const calls = [];
  const api = {
    runtime: { id: "id", getURL: (file) => `moz-extension://id/${file}`, onMessage: { addListener: (fn) => { listener = fn; } } },
    tabs: { query: async () => [tab], get: async () => tab },
    permissions: { contains: async () => true },
    storage: { local: { get: async () => settings } },
    scripting: { executeScript: async ({ func, target }) => {
      assert.equal(func, collectMedia);
      if (denyAllFrames && target.allFrames) throw new Error("Restricted child frame");
      calls.push("scan");
      if (frames) { assert.equal(target.allFrames, true); return frames; }
      return [{ result: { title: "Page title", entries: [
        { url: "/film.m3u8?signature=ephemeral", source: "video", group: 0 },
        { url: "/sub.vtt", source: "track", group: 0 },
      ] } }];
    } },
  };
  installMediaPanel(api, async (message) => { calls.push(message); return reply; });
  const sender = { id: "id", url: api.runtime.getURL("options.html") };
  const send = (message) => new Promise((resolve) => listener(message, sender, resolve));
  return { api, calls, send, listener: (...args) => listener(...args), navigate: () => { tab = { ...tab, url: "https://page.example/other" }; } };
}

test("offer requires a scan and selected authorized candidate; no webpage URL or cookies reach native", async () => {
  const h = harness();
  assert.equal(h.listener({ op: "offer_media", url: "https://evil.example" }, { id: "id", url: "https://page.example", tab: { id: 7 } }, () => assert.fail()), undefined);
  assert.equal((await h.send({ op: "offer_media", index: 0 })).error, "rescan");
  const scanned = await h.send({ op: "scan_media" });
  assert.equal(scanned.offers.length, 1);
  assert.deepEqual(h.calls, ["scan"]);
  assert.equal((await h.send({ op: "offer_media", index: 5 })).error, "rescan");
  assert.equal((await h.send({ op: "offer_media", index: 0, subtitleIndex: 4 })).error, "rescan");
  assert.equal((await h.send({ op: "offer_media", index: 0, subtitleIndex: 0, url: "https://evil.example", headers: { cookie: "forged" } })).ok, true);
  assert.deepEqual(h.calls[1], { op: "offer_media", kind: "hls", url: "https://page.example/film.m3u8?signature=ephemeral", title: "Page title", subtitle_url: "https://page.example/sub.vtt" });
  assert.equal((await h.send({ op: "offer_media", index: 0 })).error, "rescan");
});

test("navigation, disabled integration and missing native host fail without touching browser media", async () => {
  const h = harness({ reply: { ok: false, error: "disabled" } });
  await h.send({ op: "scan_media" });
  assert.deepEqual(await h.send({ op: "offer_media", index: 0 }), { ok: false, error: "unavailable" });
  await h.send({ op: "scan_media" });
  h.navigate();
  assert.equal((await h.send({ op: "offer_media", index: 0 })).error, "rescan");
  const disabled = harness({ settings: { ...mediaDefaults, mediaEnabled: false } });
  assert.equal((await disabled.send({ op: "scan_media" })).error, "disabled");
  assert.deepEqual(disabled.calls, []);
});

test("all-frame scanning resolves each frame against its own base and never cross-associates captions", async () => {
  const h = harness({ frames: ["one", "two"].map((name) => ({ result: { title: name, base: `https://${name}.example/player/`, entries: [
    { url: "film.mp4", source: "video", group: 0 }, { url: "sub.vtt", source: "track", group: 0 },
  ] } })) });
  const { offers } = await h.send({ op: "scan_media" });
  assert.equal(offers.length, 2);
  for (const [index, name] of ["one", "two"].entries()) {
    assert.equal(offers[index].url, `https://${name}.example/player/film.mp4`);
    assert.deepEqual(offers[index].subtitles.map((track) => track.url), [`https://${name}.example/player/sub.vtt`]);
  }
});

test("a denied child-frame injection does not discard the permitted top-frame scan", async () => {
  const h = harness({ denyAllFrames: true });
  assert.equal((await h.send({ op: "scan_media" })).offers.length, 1);
});

test("floating offers are bound to the requesting tab/frame and cannot hand off page-supplied URLs", async () => {
  const h = harness({ settings: { ...mediaDefaults, floatingVideos: true } });
  const sender = { id: "id", tab: { id: 7, url: "https://page.example/player" }, frameId: 0, url: "https://page.example/player" };
  const send = (message, source = sender) => new Promise((resolve) => h.listener(message, source, resolve));
  const scanned = await send({ op: "video_offers", url: "https://untrusted.example/forged" });
  assert.equal(scanned.ok, true);
  assert.equal((await send({ op: "offer_video", token: scanned.token, index: 0 }, { ...sender, frameId: 3 })).error, "rescan");
  assert.equal(h.calls.filter((call) => typeof call === "object").length, 0);
  const next = await send({ op: "video_offers" });
  assert.equal((await send({ op: "offer_video", token: next.token, index: 0, url: "https://untrusted.example/forged" })).ok, true);
  assert.equal(h.calls.find((call) => typeof call === "object").url, "https://page.example/film.m3u8?signature=ephemeral");
  assert.equal((await send({ op: "offer_video", token: next.token, index: 0 })).error, "rescan");
});
