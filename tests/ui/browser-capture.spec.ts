import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

async function bootstrap(page: Page, failure = false) {
  await page.addInitScript(({ failure }) => {
    localStorage.setItem("osdm.settings", JSON.stringify({ state: { settings: { theme: "dark", onboarding_complete: true, desktop: { silent: true } } }, version: 0 }));
    const calls: Array<{ command: string; args: Record<string, unknown> }> = [];
    let pending = true;
    const download = { id: "10000000-0000-4000-8000-000000000001", url: "https://files.example/captured.zip", destination: "C:\\Downloads\\captured.zip", temporary_path: "C:\\Downloads\\fixture.part", state: "Paused", total_bytes: null, segments: [], error: null, bytes_per_second: 0, eta_seconds: null, validator: null, ranged: false };
    Object.assign(window, { __captureCalls: calls, __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} }, __TAURI_INTERNALS__: {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } }, transformCallback() { return 1; }, unregisterCallback() {},
      async invoke(command: string, args: Record<string, unknown> = {}) {
        calls.push({ command, args });
        switch (command) {
          case "settings_load": return null;
          case "settings_save": case "engine_open": case "engine_shutdown": case "plugin:event|unlisten": return;
          case "plugin:event|listen": return 1;
          case "engine_snapshot": return [download];
          case "capture_reviews": return pending ? [download] : [];
          case "capture_review": if (failure) throw new Error("Fixture failure"); pending = false; return;
          case "engine_update_destination": download.destination = args.destination as string; return;
          case "engine_media_jobs": case "media_offers": return [];
          case "engine_automation": return { queues: [], schedules: [], membership: {} };
          case "capture_status": return true;
          case "power_status": return null;
          case "desktop_autostart_status": return false;
          default: throw new Error(`Unstubbed command ${command}`);
        }
      },
    } });
  }, { failure });
  await page.goto("/");
  return page.getByRole("dialog", { name: "Download captured from browser" });
}
async function reviews(page: Page) { return page.evaluate(() => (window as any).__captureCalls.filter((call: any) => call.command === "capture_review")); }
test("intercept opens a confirmation without enqueue/probe/start, then starts exactly once after confirmation", async ({ page }) => {
  const dialog = await bootstrap(page);
  await expect(dialog).toBeVisible();
  expect(await reviews(page)).toEqual([]);
  expect(await page.evaluate(() => (window as any).__captureCalls.filter((call: any) => ["engine_enqueue", "engine_probe", "engine_resume"].includes(call.command)))).toEqual([]);
  await dialog.getByLabel("Save as", { exact: true }).fill("C:\\Downloads\\chosen.zip");
  await dialog.getByRole("button", { name: "Start download", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(await reviews(page)).toEqual([{ command: "capture_review", args: { id: "10000000-0000-4000-8000-000000000001", action: "start" } }]);
});
for (const action of ["later", "discard", "escape"] as const) test(`${action} never starts the intercepted file`, async ({ page }) => {
  const dialog = await bootstrap(page);
  await expect(dialog).toBeVisible();
  if (action === "escape") await page.keyboard.press("Escape");
  else await dialog.getByRole("button", { name: action === "later" ? "Download later" : "Discard capture", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect((await reviews(page)).map((call: any) => call.args.action)).toEqual([action === "escape" ? "later" : action]);
});
test("failed review remains open, retryable and accessible", async ({ page }) => {
  const dialog = await bootstrap(page, true);
  await dialog.getByRole("button", { name: "Start download", exact: true }).click();
  await expect(dialog).toContainText("Check the destination and queue before retrying");
  await expect(dialog.getByRole("button", { name: "Start download", exact: true })).toBeEnabled();
  expect((await new AxeBuilder({ page }).include('[role="dialog"]').analyze()).violations).toEqual([]);
});
