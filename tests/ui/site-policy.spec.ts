import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// Fake-IPC contract tests use the real TauriBridge. They do not claim native
// crawling; engine integration tests cover real loopback requests separately.
async function bootstrap(page: Page, theme = "dark") {
  await page.emulateMedia({ colorScheme: theme as "dark" | "light", reducedMotion: "reduce" });
  await page.addInitScript(({ theme }) => {
    localStorage.setItem("osdm.settings", JSON.stringify({ state: { settings: { theme, onboarding_complete: true, desktop: { silent: true } } }, version: 0 }));
    const calls: Array<{ command: string; args: Record<string, unknown> }> = [];
    const project = {
      id: "11111111-1111-4111-8111-111111111111", name: "Fixture site", destination: "C:\\FakeDownloads",
      state: "Ready", pages: 1, bytes: 100, generation: 0, last_run: null, schedule_note: null,
      extensions: ["pdf"], excluded_paths: [], resources: [], error: null,
      policy: { automatic_downloads: false, max_concurrent_downloads: 1, cache_responses: false, next_run: null, interval_seconds: null, catch_up_on_startup: false },
    };
    Object.assign(window, { __sitePolicyCalls: calls, __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} }, __TAURI_INTERNALS__: {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback() { return 1; }, unregisterCallback() {},
      async invoke(command: string, args: Record<string, unknown> = {}) {
        calls.push({ command, args });
        switch (command) {
          case "settings_load": return null;
          case "settings_save": case "engine_open": case "engine_shutdown": case "plugin:event|unlisten": return;
          case "plugin:event|listen": return 1;
          case "engine_snapshot": case "engine_media_jobs": case "media_offers": return [];
          case "engine_automation": return { queues: [], schedules: [], membership: {} };
          case "power_status": return null;
          case "capture_status": case "desktop_autostart_status": return false;
          case "engine_site_projects": return [structuredClone(project)];
          case "engine_configure_site_policy": project.policy = args.policy as typeof project.policy; return;
          case "engine_update_site_filters":
            project.extensions = args.extensions as string[];
            project.excluded_paths = args.excludedPaths as string[];
            project.state = "Paused"; project.generation += 1;
            project.policy = { ...project.policy, automatic_downloads: false, next_run: null, interval_seconds: null };
            return;
          case "engine_control_site_project": if (args.action === "recheck") project.generation += 1; return;
          default: throw new Error(`Unstubbed IPC fixture command: ${command}`);
        }
      },
    } });
  }, { theme });
  await page.goto("/");
  await page.getByRole("button", { name: "Site Grabber", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Site Grabber", exact: true });
  await dialog.getByRole("button", { name: /^Fixture site/ }).click();
  await expect(dialog.getByRole("heading", { name: "Fixture site", exact: true })).toBeVisible();
  return dialog;
}

async function calls(page: Page, name: string) {
  return page.evaluate((name) => (window as unknown as { __sitePolicyCalls: Array<{ command: string; args: Record<string, unknown> }> }).__sitePolicyCalls.filter((call) => call.command === name), name);
}

test("site automation and UTC scheduling require fresh consent and retain the native wire contract", async ({ page }) => {
  const dialog = await bootstrap(page);
  const opener = dialog.getByRole("button", { name: "Schedule, automation & filters", exact: true });
  await opener.click();
  const editor = dialog.getByRole("region", { name: "Site project settings", exact: true });
  const save = editor.getByRole("button", { name: "Save project policy", exact: true });
  await expect(save).toBeDisabled();
  await editor.getByRole("checkbox", { name: /Automatically queue/ }).check();
  await editor.getByRole("checkbox", { name: /always-revalidated local page cache/ }).check();
  await editor.getByLabel("Active resource downloads (1–32)", { exact: true }).fill("2");
  await editor.getByLabel("Next recheck (UTC)", { exact: true }).fill("2100-01-01T04:00");
  await editor.getByLabel("Repeat every (minutes)", { exact: true }).fill("60");
  await editor.getByRole("checkbox", { name: /Coalesce a missed run/ }).check();
  const consent = editor.getByRole("checkbox", { name: /I authorize this unattended policy/ });
  await consent.check();
  await editor.getByLabel("Active resource downloads (1–32)", { exact: true }).fill("3");
  await expect(consent).not.toBeChecked();
  await expect(save).toBeDisabled();
  await consent.check(); await save.click();
  await expect(editor).toHaveCount(0);
  await expect(opener).toBeFocused();
  const recorded = await calls(page, "engine_configure_site_policy");
  expect(recorded).toHaveLength(1);
  expect(recorded[0].args).toEqual({ id: "11111111-1111-4111-8111-111111111111", confirmed: true, policy: {
    automatic_downloads: true, max_concurrent_downloads: 3, cache_responses: true,
    next_run: Date.parse("2100-01-01T04:00:00Z") / 1000, interval_seconds: 3600, catch_up_on_startup: true,
  } });
  await expect(dialog).toContainText("Automatic matching downloads enabled");
  await expect(dialog).toContainText("2100-01-01 04:00:00 UTC");
});

test("filter continuation is explicit and Recheck now uses its own action", async ({ page }) => {
  const dialog = await bootstrap(page);
  await dialog.getByRole("button", { name: "Schedule, automation & filters", exact: true }).click();
  const editor = dialog.getByRole("region", { name: "Site project settings", exact: true });
  const save = editor.getByRole("button", { name: "Save filters & pause", exact: true });
  await expect(save).toBeDisabled();
  await editor.getByLabel("New allowed file extensions", { exact: true }).fill("png jpg");
  await editor.getByLabel("New excluded path prefixes", { exact: true }).fill("/docs/private");
  await editor.getByRole("checkbox", { name: /I confirm these filter changes/ }).check();
  await save.click();
  await expect.poll(() => calls(page, "engine_update_site_filters")).toHaveLength(1);
  expect((await calls(page, "engine_update_site_filters"))[0].args).toEqual({ id: "11111111-1111-4111-8111-111111111111", extensions: ["png", "jpg"], excludedPaths: ["/docs/private"], confirmed: true });
  await expect(editor).toHaveCount(0);
  await dialog.getByRole("button", { name: "Recheck now", exact: true }).click();
  await expect.poll(() => calls(page, "engine_control_site_project")).toHaveLength(1);
  expect((await calls(page, "engine_control_site_project")).at(-1)?.args.action).toBe("recheck");
  expect(await calls(page, "engine_queue_site_resources")).toEqual([]);
});

for (const theme of ["dark", "light"]) {
  test(`${theme} site policy editor remains accessible`, async ({ page }) => {
    const dialog = await bootstrap(page, theme);
    await dialog.getByRole("button", { name: "Schedule, automation & filters", exact: true }).click();
    const scan = await new AxeBuilder({ page }).include('[role="dialog"]').analyze();
    expect(scan.violations).toEqual([]);
  });
}
