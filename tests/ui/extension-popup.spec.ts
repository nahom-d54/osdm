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

async function bootstrap(page: Page, { permission = true, theme = "dark" } = {}) {
  await page.setViewportSize({ width: 400, height: 780 });
  await page.addInitScript(({ permission, theme }) => {
    const calls: Array<{ op: string; value?: unknown }> = [];
    const listeners: Array<(changes: object, area: string) => void> = [];
    const settings: Record<string, unknown> = { theme };
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
  }, { permission, theme });
  await page.goto(`${base}/options.html#/capture`);
  await expect(page.getByRole("switch", { name: "Capture browser downloads" })).toBeVisible();
}
test("hash-routed React capture UI includes cookies by default and asks once at the enable gesture", async ({ page }) => {
  await bootstrap(page);
  await expect(page.getByRole("switch", { name: "Include login cookies" })).toBeChecked();
  await page.getByRole("switch", { name: "Capture browser downloads" }).check();
  await expect(page.getByRole("heading", { name: "Automatic capture is on" })).toBeVisible();
  const calls = await page.evaluate(() => (window as any).__popup.calls);
  expect(calls[0]).toEqual({ op: "permission", value: { permissions: ["cookies"], origins: ["https://*/*"] } });
  expect(calls[1]).toEqual({ op: "save", value: { enabled: true } });
  await page.getByRole("link", { name: "Rules", exact: true }).click();
  await expect(page).toHaveURL(/#\/rules$/);
  await expect(page.getByLabel("Captured file types")).toHaveValue(/apk/);
  await page.goBack();
  await expect(page.getByRole("switch", { name: "Capture browser downloads" })).toBeChecked();
});
test("declined cookie permission offers the global opt-out without ever reading or saving cookie values", async ({ page }) => {
  await bootstrap(page, { permission: false });
  await page.getByRole("switch", { name: "Capture browser downloads" }).click();
  await expect(page.getByRole("alert")).toContainText("Cookie permission was declined");
  await expect(page.getByRole("switch", { name: "Capture browser downloads" })).not.toBeChecked();
  await page.getByRole("switch", { name: "Include login cookies" }).uncheck();
  await page.getByRole("switch", { name: "Capture browser downloads" }).check();
  await expect(page.getByRole("switch", { name: "Capture browser downloads" })).toBeChecked();
  expect(await page.evaluate(() => (window as any).__popup.settings)).toMatchObject({ enabled: true, cookiesEnabled: false });
});
test("video page enables the floating script only after a user permission grant and offers one reviewed source", async ({ page }) => {
  await bootstrap(page);
  await page.getByRole("link", { name: "Video", exact: true }).click();
  await page.getByRole("switch", { name: "Floating video download button" }).check();
  await expect(page.getByRole("status")).toContainText("Reload already-open pages");
  expect((await page.evaluate(() => (window as any).__popup.calls)).some((call: any) => call.op === "scripts")).toBe(true);
  await page.getByRole("button", { name: "Scan current page" }).click();
  await page.getByRole("radio").check();
  await page.getByRole("button", { name: "Review download in OSDM" }).click();
  await expect(page.getByRole("status")).toContainText("Sent to OSDM");
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
