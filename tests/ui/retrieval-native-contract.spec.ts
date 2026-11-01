import { test, expect, type Page } from "@playwright/test";

// Fake IPC contract coverage through the production TauriBridge, not a native
// end-to-end test. All URLs and filesystem paths below are synthetic fixtures.
type Offer = { id: string; kind: "hls" | "dash" | "direct"; url: string; title: string; subtitle_url: string | null };
type Call = { command: string; args: Record<string, unknown> };
type IpcFixture = {
  calls: Call[];
  unexpected: string[];
  pendingPaths: number;
  resolvePaths(exists: boolean): Promise<void>;
};
declare global {
  interface Window { __retrievalIpc: IpcFixture }
}

async function bootstrap(page: Page, offers: Offer[] = []) {
  await page.emulateMedia({ reducedMotion: "reduce", colorScheme: "dark" });
  await page.addInitScript((initialOffers) => {
    localStorage.setItem("osdm.settings", JSON.stringify({ state: { settings: {
      theme: "dark", onboarding_complete: true, desktop: { silent: true },
      default_directory: "C:\\FakeDownloads",
    } }, version: 0 }));
    const calls: Call[] = [];
    const unexpected: string[] = [];
    const paths: Array<(exists: boolean) => void> = [];
    const callbacks = new Map<number, (payload: unknown) => void>();
    let callbackId = 0;
    let eventId = 0;
    let offers = initialOffers;
    const fixture: IpcFixture = {
      calls, unexpected,
      get pendingPaths() { return paths.length; },
      async resolvePaths(exists) {
        paths.splice(0).forEach((resolve) => resolve(exists));
        // Flush promise continuations and React's resulting tasks before a
        // negative enqueue assertion, rather than checking too early.
        await new Promise((resolve) => setTimeout(resolve, 0));
        await new Promise((resolve) => setTimeout(resolve, 0));
      },
    };
    window.__retrievalIpc = fixture;
    Object.assign(window, {
      __TAURI_INTERNALS__: {
        metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
        transformCallback(callback: (payload: unknown) => void, once = false) {
          const id = ++callbackId;
          callbacks.set(id, (payload) => { if (once) callbacks.delete(id); callback(payload); });
          return id;
        },
        unregisterCallback(id: number) { callbacks.delete(id); },
        runCallback(id: number, payload: unknown) { callbacks.get(id)?.(payload); },
        convertFileSrc(path: string) { return `asset://localhost/${encodeURIComponent(path)}`; },
        async invoke(command: string, args: Record<string, unknown> = {}) {
          calls.push({ command, args });
          switch (command) {
            case "settings_load": return null;
            case "settings_save": case "engine_open": case "engine_shutdown":
            case "engine_cancel_inspection": case "plugin:event|unlisten": return;
            case "plugin:event|listen": return ++eventId;
            case "engine_snapshot": case "engine_media_jobs": case "engine_site_projects": return [];
            case "engine_automation": return { queues: [], schedules: [], membership: {} };
            case "power_status": return null;
            case "capture_status": case "desktop_autostart_status": return false;
            case "default_download_directory": return "C:\\FakeDownloads";
            case "media_offers": return offers;
            case "media_dismiss_offer": offers = offers.filter((offer) => offer.id !== args.id); return;
            case "path_exists": return new Promise<boolean>((resolve) => paths.push(resolve));
            case "engine_enqueue": return "fixture-download";
            case "engine_create_media_job": return "fixture-media-job";
            case "engine_inspect_media":
              return args.url === "https://media.example.test/master.m3u8" ? {
                kind: "master", choices: [{ url: "https://cdn.example.test/variant.m3u8", bandwidth: 1400000, resolution: [1280, 720], codecs: "avc1.64001f" }],
                segment_count: 0, resource_origins: ["https://cdn.example.test"],
              } : {
                kind: "vod", choices: [], segment_count: 2, resource_origins: ["https://cdn.example.test"],
              };
            default:
              unexpected.push(command);
              throw new Error(`Unstubbed fake IPC command: ${command}`);
          }
        },
      },
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
    });
  }, offers);
  await page.goto("/");
  await expect.poll(() => commands(page, "engine_snapshot")).toHaveLength(1);
  await expect.poll(() => commands(page, "plugin:event|listen")).toHaveLength(2);
  await expect(page.getByRole("alert")).toHaveCount(0);
}

async function commands(page: Page, command: string): Promise<Call[]> {
  return page.evaluate((name) => window.__retrievalIpc.calls.filter((call) => call.command === name), command);
}

async function directMedia(page: Page) {
  await page.getByRole("button", { name: "Save media", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Save media", exact: true });
  await dialog.getByRole("radio", { name: "Direct media file", exact: true }).check();
  await dialog.getByLabel("Media URL", { exact: true }).fill("https://media.example.test/film.mp4");
  await dialog.getByLabel("Output folder", { exact: true }).fill("C:\\FakeDownloads");
  await dialog.getByLabel("Filename", { exact: true }).fill("film.mp4");
  return dialog;
}

test.afterEach(async ({ page }) => {
  if (await page.evaluate(() => !!window.__retrievalIpc))
    expect(await page.evaluate(() => window.__retrievalIpc.unexpected)).toEqual([]);
});

for (const closeMethod of ["Escape", "Close button"] as const) {
  test(`closing with ${closeMethod} during deferred native preflight prevents later admission`, async ({ page }) => {
    await bootstrap(page);
    const dialog = await directMedia(page);
    await dialog.getByRole("button", { name: "Save selected media", exact: true }).click();
    await expect.poll(() => page.evaluate(() => window.__retrievalIpc.pendingPaths)).toBe(1);
    expect(await commands(page, "path_exists")).toEqual([{ command: "path_exists", args: { path: "C:\\FakeDownloads\\film.mp4" } }]);
    expect(await commands(page, "engine_enqueue")).toEqual([]);
    if (closeMethod === "Escape") await page.keyboard.press("Escape");
    else await dialog.locator('[data-slot="dialog-footer"]').getByRole("button", { name: "Close", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await page.evaluate(() => window.__retrievalIpc.resolvePaths(false));
    expect(await commands(page, "engine_enqueue")).toEqual([]);
    expect(await commands(page, "engine_create_media_job")).toEqual([]);
    await expect(page.locator("li[data-row]")).toHaveCount(0);
    await page.getByRole("button", { name: "Save media", exact: true }).click();
    await expect(dialog.getByLabel("Media URL", { exact: true })).toHaveValue("");
    await expect(dialog).not.toContainText("Media added to downloads.");
  });
}

test("pending save locks editable fields and synchronous duplicate clicks admit only once", async ({ page }) => {
  await bootstrap(page);
  const dialog = await directMedia(page);
  const save = dialog.getByRole("button", { name: "Save selected media", exact: true });
  // Same-turn clicks exercise savingRef before React has committed disabled=true.
  await save.evaluate((button: HTMLButtonElement) => { button.click(); button.click(); });
  await expect.poll(() => page.evaluate(() => window.__retrievalIpc.pendingPaths)).toBe(1);
  await expect(save).toBeDisabled();
  for (const name of ["Media URL", "Output folder", "Filename", "Description", "Optional selected subtitle URL"])
    await expect(dialog.getByLabel(name, { exact: true })).toBeDisabled();
  for (const name of ["HLS stream", "DASH stream", "Direct media file"])
    await expect(dialog.getByRole("radio", { name, exact: true })).toBeDisabled();
  await expect(dialog.getByRole("button", { name: "Choose media output folder", exact: true })).toBeDisabled();
  await save.evaluate((button: HTMLButtonElement) => button.click());
  expect(await commands(page, "path_exists")).toHaveLength(1);
  expect(await commands(page, "engine_enqueue")).toEqual([]);
  await page.evaluate(() => window.__retrievalIpc.resolvePaths(false));
  await expect.poll(() => commands(page, "engine_enqueue")).toHaveLength(1);
  await expect(dialog.getByRole("status")).toContainText("Media added to downloads.");
  await expect(save).toBeEnabled();
  expect((await commands(page, "engine_enqueue"))[0].args).toMatchObject({
    request: { url: "https://media.example.test/film.mp4", destination: "C:\\FakeDownloads\\film.mp4", expected_sha256: null },
    startPaused: false,
  });
  expect(await commands(page, "engine_create_media_job")).toEqual([]);
});

for (const url of ["https://media.example.test/manifest.mpd", "https://media.example.test/manifest?format=dash"]) {
  test(`review preserves DASH offer kind (${url.endsWith(".mpd") ? "manifest suffix" : "extensionless endpoint"}) without direct-file admission`, async ({ page }) => {
    await bootstrap(page, [{ id: "dash-offer", kind: "dash", url, title: "Fixture DASH", subtitle_url: null }]);
    await page.getByRole("button", { name: "Choose quality & destination", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Save media", exact: true });
    await expect(dialog.getByRole("radio", { name: "DASH stream", exact: true })).toBeChecked();
    await expect(dialog.getByRole("radio", { name: "Direct media file", exact: true })).not.toBeChecked();
    await expect(dialog.getByLabel("Media URL", { exact: true })).toHaveValue(url);
    // No assertion that DASH must remain unsupported: when integrated, it still
    // must require explicit stream inspection/selection before admission.
    const save = dialog.getByRole("button", { name: "Save selected media", exact: true });
    await expect(save).toBeDisabled();
    await save.evaluate((button: HTMLButtonElement) => button.click());
    await dialog.locator('[data-slot="dialog-footer"]').getByRole("button", { name: "Close", exact: true }).click();
    expect(await commands(page, "engine_enqueue")).toEqual([]);
    expect(await commands(page, "engine_create_media_job")).toEqual([]);
    expect(await commands(page, "path_exists")).toEqual([]);
  });
}

test("HLS offer preloads its selected subtitle and propagates it with explicit approved origins", async ({ page }) => {
  const subtitle = "https://captions.example.test/en.vtt";
  await bootstrap(page, [{ id: "hls-offer", kind: "hls", url: "https://media.example.test/master.m3u8", title: "Fixture HLS", subtitle_url: subtitle }]);
  await page.getByRole("button", { name: "Choose quality & destination", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Save media", exact: true });
  await expect(dialog.getByRole("radio", { name: "HLS stream", exact: true })).toBeChecked();
  await expect(dialog.getByLabel("Optional selected subtitle URL", { exact: true })).toHaveValue(subtitle);
  await dialog.getByLabel("Additional approved origins", { exact: true }).fill("https://cdn.example.test\nhttps://captions.example.test");
  await dialog.getByRole("button", { name: "Inspect manifest", exact: true }).click();
  const quality = dialog.getByRole("group", { name: "Choose a quality", exact: true });
  await expect(quality.getByRole("radio")).toHaveCount(1);
  await expect(quality).toContainText("1280 × 720");
  await quality.getByRole("radio").check();
  await dialog.getByRole("button", { name: "Inspect selected quality", exact: true }).click();
  await expect(dialog.getByRole("status")).toContainText("2 ordered segments");
  const inspections = await commands(page, "engine_inspect_media");
  expect(inspections).toHaveLength(2);
  expect(inspections[1].args).toMatchObject({ url: "https://cdn.example.test/variant.m3u8", allowedOrigins: ["https://cdn.example.test", "https://captions.example.test"], requestId: expect.any(String) });
  await dialog.getByLabel("Filename", { exact: true }).fill("fixture.mkv");
  await dialog.getByLabel("Installed FFmpeg executable", { exact: true }).fill("C:\\Tools\\ffmpeg.exe");
  await dialog.getByRole("button", { name: "Save selected media", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__retrievalIpc.pendingPaths)).toBe(1);
  expect(await commands(page, "engine_create_media_job")).toEqual([]);
  await page.evaluate(() => window.__retrievalIpc.resolvePaths(false));
  await expect.poll(() => commands(page, "engine_create_media_job")).toHaveLength(1);
  expect((await commands(page, "engine_create_media_job"))[0].args).toEqual({ request: {
    url: "https://cdn.example.test/variant.m3u8", destination: "C:\\FakeDownloads\\fixture.mkv",
    allowed_origins: ["https://cdn.example.test", "https://captions.example.test"],
    ffmpeg_path: "C:\\Tools\\ffmpeg.exe", description: "Fixture HLS", subtitle_url: subtitle,
    category_id: null, dash_tracks: [],
  } });
  await expect.poll(() => commands(page, "media_dismiss_offer")).toEqual([{ command: "media_dismiss_offer", args: { id: "hls-offer" } }]);
  expect(await commands(page, "engine_enqueue")).toEqual([]);
});
