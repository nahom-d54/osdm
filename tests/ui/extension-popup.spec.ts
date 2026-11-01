import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import http from "node:http";
// @ts-expect-error Build helper is intentionally shared with the Node packaging CLI.
import { stageExtension } from "../../extension/release.mjs";

let server: http.Server;
let base: string;
let directory: string;
test.beforeAll(async () => {
  const parent = process.env.LOCALAPPDATA ? path.join(process.env.LOCALAPPDATA, "Temp", "opencode") : os.tmpdir();
  await fs.mkdir(parent, { recursive: true });
  directory = await fs.mkdtemp(path.join(parent, "osdm-popup-ui-"));
  const source = path.join(directory, "extension");
  await stageExtension("chromium", "0.1.1", source);
  server = http.createServer(async (request, response) => {
    const pathname = new URL(request.url!, "http://localhost").pathname;
    const file = path.join(source, pathname);
    if (!file.startsWith(source + path.sep)) { response.writeHead(403).end(); return; }
    try {
      const bytes = await fs.readFile(file);
      response.setHeader("Content-Type", ({ ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".png": "image/png", ".woff2": "font/woff2", ".woff": "font/woff" } as Record<string, string>)[path.extname(file)] || "application/octet-stream");
      response.end(bytes);
    } catch { response.writeHead(404).end(); }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as import("node:net").AddressInfo).port}`;
});
test.afterAll(async () => { server?.closeAllConnections(); await new Promise<void>((resolve) => server?.close(() => resolve())); });

async function bootstrap(page: Page, { permission = true, theme = "dark", disabled = false } = {}) {
  await page.setViewportSize({ width: 460, height: 780 });
  await page.addInitScript(({ permission, theme, disabled }) => {
    const calls: Array<{ op: string; value?: unknown }> = [];
    const listeners: Array<(changes: object, area: string) => void> = [];
    const settings: Record<string, unknown> = { theme, ...(disabled ? { enabled: false, cookiesEnabled: false, floatingVideos: false, networkMediaEnabled: false } : {}) };
    const api = {
      runtime: { id: "test-id", getURL: (file: string) => `${location.origin}/${file}`,
        sendNativeMessage: async () => ({ ok: true, version: 1 }),
        sendMessage: async (message: { op: string }) => { calls.push({ op: message.op, value: message }); return ["scan_media", "video_offers"].includes(message.op) ? { ok: true, token: "fixture-token", offers: [{ url: "https://media.example/film.m3u8", kind: "HLS", title: "Fixture film", subtitles: [] }] } : { ok: true }; } },
      tabs: { query: async () => [{ id: 7, url: "https://page.example/player", title: "Player" }] },
      storage: { onChanged: { addListener: (listener: (changes: object, area: string) => void) => listeners.push(listener) }, local: { get: async (defaults: object) => ({ ...defaults, ...settings }), set: async (patch: object) => { Object.assign(settings, patch); calls.push({ op: "save", value: patch }); const changes = Object.fromEntries(Object.entries(patch).map(([key, value]) => [key, { newValue: value }])); for (const listener of listeners) listener(changes, "local"); } }, session: { get: async (defaults: object) => defaults, remove: async () => {} } },
      permissions: { contains: async () => permission, request: async (value: object) => { calls.push({ op: "permission", value }); return permission; } },
      scripting: { unregisterContentScripts: async () => {}, registerContentScripts: async (value: object) => { calls.push({ op: "scripts", value }); } },
      action: { setBadgeText: async () => {} },
    };
    Object.assign(window, { browser: api, __popup: { settings, calls } });
  }, { permission, theme, disabled });
  await page.goto(`${base}/options.html#/capture`);
  await expect(page.getByRole("switch", { name: "Capture browser downloads" })).toBeVisible();
}
test("hash-routed capture defaults on with cookies and preserves the global opt-out", async ({ page }) => {
  await bootstrap(page);
  await expect(page.getByRole("switch", { name: "Include login cookies" })).toBeChecked();
  await expect(page.getByRole("switch", { name: "Capture browser downloads" })).toBeChecked();
  await expect(page.locator("body")).toHaveCSS("width", "460px");
  await page.getByRole("switch", { name: "Capture browser downloads" }).uncheck();
  await page.getByRole("switch", { name: "Capture browser downloads" }).check();
  await expect(page.getByRole("heading", { name: "Automatic capture is on" })).toBeVisible();
  const calls = await page.evaluate(() => (window as any).__popup.calls);
  expect(calls[1]).toEqual({ op: "permission", value: { permissions: ["cookies"], origins: ["https://*/*"] } });
  expect(calls[2]).toEqual({ op: "save", value: { enabled: true } });
  await page.getByRole("link", { name: "Rules", exact: true }).click();
  await expect(page).toHaveURL(/#\/rules$/);
  await expect(page.getByLabel("Captured file types")).toHaveValue(/apk/);
  await page.goBack();
  await expect(page.getByRole("switch", { name: "Capture browser downloads" })).toBeChecked();
});
test("declined cookie permission offers the global opt-out without ever reading or saving cookie values", async ({ page }) => {
  await bootstrap(page, { permission: false });
  await page.getByRole("switch", { name: "Capture browser downloads" }).click();
  await page.getByRole("switch", { name: "Capture browser downloads" }).click();
  await expect(page.getByRole("alert")).toContainText("Cookie permission was declined");
  await expect(page.getByRole("switch", { name: "Capture browser downloads" })).not.toBeChecked();
  await page.getByRole("switch", { name: "Include login cookies" }).uncheck();
  await page.getByRole("switch", { name: "Capture browser downloads" }).check();
  await expect(page.getByRole("switch", { name: "Capture browser downloads" })).toBeChecked();
  expect(await page.evaluate(() => (window as any).__popup.settings)).toMatchObject({ enabled: true, cookiesEnabled: false });
});
test("video defaults on for every permitted site and still supports opt-out and reviewed sources", async ({ page }) => {
  await bootstrap(page);
  await page.getByRole("link", { name: "Video", exact: true }).click();
  await expect(page.getByRole("switch", { name: "Floating video download button" })).toBeChecked();
  await expect(page.getByRole("switch", { name: "Network media discovery" })).toBeChecked();
  await page.getByRole("switch", { name: "Floating video download button" }).uncheck();
  await page.getByRole("switch", { name: "Floating video download button" }).check();
  await expect(page.getByRole("status")).toContainText("enabled on all permitted sites");
  await page.getByRole("button", { name: "Scan current page" }).click();
  await page.getByRole("radio").check();
  await page.getByRole("button", { name: "Review download in OSDM" }).click();
  await expect(page.getByRole("status")).toContainText("Sent to OSDM");
});

test("default-on update preserves explicit saved capture, cookie and video opt-outs", async ({ page }) => {
  await bootstrap(page, { disabled: true });
  await expect(page.getByRole("switch", { name: "Capture browser downloads" })).not.toBeChecked();
  await expect(page.getByRole("switch", { name: "Include login cookies" })).not.toBeChecked();
  await page.getByRole("link", { name: "Video", exact: true }).click();
  await expect(page.getByRole("switch", { name: "Floating video download button" })).not.toBeChecked();
  await expect(page.getByRole("switch", { name: "Network media discovery" })).not.toBeChecked();
});
for (const theme of ["dark", "light"]) test(`${theme} extension navigation and capture controls are accessible`, async ({ page }) => {
  await bootstrap(page, { theme });
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.screenshot({ path: path.join(directory, `${theme}-capture.png`), fullPage: true });
});

test("floating player control requires a real user click, offers a source choice, and disappears on opt-out", async ({ page }) => {
  await bootstrap(page);
  await page.evaluate(() => {
    (window as any).__popup.settings.floatingVideos = true;
    const video = document.createElement("video");
    video.style.cssText = "position:fixed;left:20px;top:100px;width:320px;height:180px";
    Object.defineProperties(video, { paused: { value: false }, ended: { value: false } });
    document.body.append(video);
  });
  await page.addScriptTag({ url: `${base}/video-button.js` });
  const control = page.locator("osdm-video-control");
  const download = control.getByRole("button", { name: "Download with OSDM" });
  await expect(download).toBeVisible();
  await download.evaluate((button: HTMLButtonElement) => button.click());
  expect((await page.evaluate(() => (window as any).__popup.calls)).filter((call: any) => call.op === "video_offers")).toHaveLength(0);
  await download.click();
  await expect(control.getByLabel("Video source to review")).toBeVisible();
  await control.getByRole("button", { name: "Review in OSDM" }).click();
  await expect(control.getByRole("status")).toContainText("Sent");
  expect((await page.evaluate(() => (window as any).__popup.calls)).find((call: any) => call.op === "offer_video").value).toEqual({ op: "offer_video", token: "fixture-token", index: 0 });
  await page.evaluate(() => (window as any).browser.storage.local.set({ floatingVideos: false }));
  await expect(control).toHaveCount(0);
});

test("default-on floating detection finds playing shadow-root videos and does not duplicate itself", async ({ page }) => {
  await bootstrap(page);
  await page.evaluate(() => {
    const player = document.createElement("fixture-player");
    const shadow = player.attachShadow({ mode: "open" });
    const video = document.createElement("video");
    video.style.cssText = "position:fixed;left:20px;top:100px;width:320px;height:180px";
    Object.defineProperties(video, { paused: { value: false }, ended: { value: false } });
    shadow.append(video); document.body.append(player);
    video.src = "https://media.example/shadow-film.mp4";
  });
  await page.addScriptTag({ url: `${base}/video-button.js` });
  await expect(page.locator("osdm-video-control").getByRole("button", { name: "Download with OSDM" })).toBeVisible();
  await page.addScriptTag({ url: `${base}/video-button.js` });
  await expect(page.locator("osdm-video-control")).toHaveCount(1);
  const entries = await page.evaluate(async () => {
    const { collectMedia } = await import(location.origin + "/media.js");
    return collectMedia().entries;
  });
  expect(entries.some((entry: { url: string }) => entry.url === "https://media.example/shadow-film.mp4")).toBe(true);
});

async function playingVideo(page: Page) {
  await page.evaluate(() => {
    const video = document.createElement("video"); video.id = "fixture-video";
    video.style.cssText = "position:fixed;left:20px;top:100px;width:320px;height:180px";
    Object.defineProperties(video, { paused: { value: false }, ended: { value: false } });
    document.body.append(video);
  });
  await page.addScriptTag({ url: `${base}/video-button.js` });
  const control = page.locator("osdm-video-control");
  await expect(control.getByRole("button", { name: "Download with OSDM" })).toBeVisible();
  return control;
}

test("floating panel can be dragged within the viewport and closed without hiding other controls", async ({ page }) => {
  await bootstrap(page);
  const control = await playingVideo(page);
  const grip = control.getByRole("button", { name: "Move video download panel" });
  const before = await control.boundingBox();
  const box = await grip.boundingBox();
  await page.mouse.move(box!.x + 4, box!.y + 4);
  await page.mouse.down();
  await page.mouse.move(box!.x + 120, box!.y + 140, { steps: 5 });
  await page.mouse.up();
  const after = await control.boundingBox();
  expect(after!.x).toBeGreaterThan(before!.x + 50);
  expect(after!.y).toBeGreaterThan(before!.y + 50);
  await control.getByRole("button", { name: "Close video download panel" }).click();
  await expect(control).toHaveCount(0);
});

test("source dropdown lists each found video without exposing query secrets", async ({ page }) => {
  await bootstrap(page);
  const control = await playingVideo(page);
  await page.evaluate(() => {
    const api = (window as any).browser;
    api.runtime.sendMessage = async () => ({ ok: true, token: "list-token", offers: [
      { url: "https://cdn.example/film.mp4?signature=secret-value", kind: "Direct media", title: "Film", subtitles: [] },
      { url: "https://cdn.example/master.m3u8?signed=secret-value", kind: "HLS", title: "Film", subtitles: [] },
    ] });
  });
  await control.getByRole("button", { name: "Download with OSDM" }).click();
  const source = control.getByLabel("Video source to review");
  await expect(source.locator("option")).toHaveCount(2);
  await expect(source).not.toContainText("secret-value");
  await expect(control.getByText("2 sources")).toBeVisible();
});

test("a player source change rejects a late scan response instead of offering the previous video", async ({ page }) => {
  await bootstrap(page);
  const control = await playingVideo(page);
  await page.evaluate(() => {
    const api = (window as any).browser;
    api.runtime.sendMessage = () => new Promise((resolve) => { (window as any).__finishVideoScan = resolve; });
  });
  await control.getByRole("button", { name: "Download with OSDM" }).click();
  await expect(control.getByRole("status")).toContainText("Finding sources");
  await page.evaluate(() => {
    const video = document.getElementById("fixture-video") as HTMLVideoElement;
    video.src = "/replacement.mp4";
    video.dispatchEvent(new Event("loadstart"));
  });
  await expect(control.locator('p[role="status"]')).toBeEmpty();
  await page.evaluate(() => (window as any).__finishVideoScan({ ok: true, token: "obsolete-token", offers: [{ url: "https://media.example/obsolete.mp4", kind: "Direct media", subtitles: [] }] }));
  await expect(control.getByRole("button", { name: "Download with OSDM" })).toBeEnabled();
  await expect(control.getByLabel("Video source to review")).toHaveCount(0);
});

test("floating review offers a selected subtitle and consumes its UI selection once", async ({ page }) => {
  await bootstrap(page);
  const control = await playingVideo(page);
  await page.evaluate(() => {
    const api = (window as any).browser;
    api.runtime.sendMessage = async (message: any) => {
      (window as any).__popup.calls.push({ op: message.op, value: message });
      return message.op === "video_offers" ? { ok: true, token: "caption-token", offers: [{ url: "https://media.example/film.mp4?secret=not-for-labels", kind: "Direct media", subtitles: [{ url: "https://media.example/en.vtt", label: "English" }] }] } : { ok: true };
    };
  });
  await control.getByRole("button", { name: "Download with OSDM" }).click();
  await expect(control.getByLabel("Video source to review")).not.toContainText("secret=");
  await control.getByLabel("Video subtitle to include").selectOption("0");
  await control.getByRole("button", { name: "Review in OSDM" }).click();
  await expect(control.getByRole("status")).toContainText("Sent");
  await expect(control.getByRole("button", { name: "Review in OSDM" })).toHaveCount(0);
  const offers = await page.evaluate(() => (window as any).__popup.calls.filter((call: any) => call.op === "offer_video"));
  expect(offers).toHaveLength(1);
  expect(offers[0].value).toEqual({ op: "offer_video", token: "caption-token", index: 0, subtitleIndex: 0 });
});

test("a changed source cannot reuse an already displayed selection", async ({ page }) => {
  await bootstrap(page);
  const control = await playingVideo(page);
  await control.getByRole("button", { name: "Download with OSDM" }).click();
  await expect(control.getByLabel("Video source to review")).toBeVisible();
  await page.evaluate(() => { (document.getElementById("fixture-video") as HTMLVideoElement).src = "/different.mp4"; });
  // The periodic refresh invalidates the displayed source list before it can be
  // reused; the old review button is removed rather than handing off stale data.
  await expect(control.getByLabel("Video source to review")).toHaveCount(0);
  await expect(control.getByRole("button", { name: "Download with OSDM" })).toBeVisible();
  expect(await page.evaluate(() => (window as any).__popup.calls.filter((call: any) => call.op === "offer_video"))).toHaveLength(0);
});

test("multiple playing videos prefer the largest visible player, then follow a trusted hover", async ({ page }) => {
  await bootstrap(page);
  await page.evaluate(() => {
    for (const [id, top, width, height] of [["small", 400, 180, 100], ["large", 100, 320, 180]] as const) {
      const video = document.createElement("video"); video.id = id;
      video.style.cssText = `position:fixed;left:20px;top:${top}px;width:${width}px;height:${height}px`;
      Object.defineProperties(video, { paused: { value: false }, ended: { value: false } }); document.body.append(video);
    }
  });
  await page.addScriptTag({ url: `${base}/video-button.js` });
  const control = page.locator("osdm-video-control");
  await expect(control).toHaveCSS("top", "110px");
  await page.locator("video#small").hover();
  await expect(control).toHaveCSS("top", "410px");
});

test("collector reads declared media metadata and preload MIME without treating embedded HTML as video", async ({ page }) => {
  await bootstrap(page);
  await page.evaluate(() => {
    for (const [property, content] of [["og:video", "https://cdn.example/clip.avi"], ["og:video:url", "https://page.example/embed"], ["og:video", "https://cdn.example/declared-video"], ["og:video:type", "video/mp4"]]) {
      const meta = document.createElement("meta"); meta.setAttribute("property", property); meta.content = content; document.head.append(meta);
    }
    const preload = document.createElement("link"); preload.rel = "preload"; preload.as = "video"; preload.type = "video/webm"; preload.href = "https://cdn.example/preloaded-source"; document.head.append(preload);
  });
  const urls = await page.evaluate(async () => {
    const { collectMedia, normalizeMedia } = await import(location.origin + "/media.js");
    return normalizeMedia(collectMedia(), location.href).map((offer: { url: string }) => offer.url);
  });
  expect(urls).toContain("https://cdn.example/clip.avi");
  expect(urls).toContain("https://cdn.example/preloaded-source");
  expect(urls).toContain("https://cdn.example/declared-video");
  expect(urls).not.toContain("https://page.example/embed");
});
