import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

type Call = { command: string; args: Record<string, unknown> };
declare global { interface Window { __browserSetup: { calls: Call[]; handshake(): void } } }
const ID = "abcdefghijklmnopabcdefghijklmnop";

async function bootstrap(page: Page, { helper = true, conflict = false, store = false, theme = "dark" } = {}) {
  await page.emulateMedia({ colorScheme: theme as "light" | "dark", reducedMotion: "reduce" });
  await page.addInitScript(({ helper, conflict, store, theme, id }) => {
    localStorage.setItem("osdm.settings", JSON.stringify({ state: { settings: { theme, onboarding_complete: true, desktop: { silent: true } } }, version: 0 }));
    const calls: Call[] = [];
    const status = { registration_supported: true, helper_available: helper, capture_enabled: false, last_handshake: null as number | null,
      browsers: ["chrome", "edge", "brave", "chromium", "firefox"].map((browser) => ({ browser, name: browser, store_url: store && browser === "chrome" ? `https://chromewebstore.google.com/detail/${id}` : null,
        extension_id: browser === "firefox" ? "osdm@osdm.app" : id,
        extension_ids: [] as string[], registered: false, conflict, unpacked_available: helper })) };
    window.__browserSetup = { calls, handshake() { status.last_handshake = 4102444800; } };
    Object.assign(window, { __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} }, __TAURI_INTERNALS__: {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } }, transformCallback() { return 1; }, unregisterCallback() {},
      async invoke(command: string, args: Record<string, unknown> = {}) {
        calls.push({ command, args });
        switch (command) {
          case "settings_load": return null;
          case "settings_save": case "engine_open": case "engine_shutdown": case "plugin:event|unlisten": return;
          case "plugin:event|listen": return 1;
          case "engine_snapshot": case "engine_media_jobs": case "media_offers": return [];
          case "engine_automation": return { queues: [], schedules: [], membership: {} };
          case "capture_status": return status.capture_enabled;
          case "power_status": return null;
          case "desktop_autostart_status": return false;
          case "browser_setup_status": return structuredClone(status);
          case "browser_setup_register": { const browser = status.browsers.find((browser) => browser.browser === args.browser)!; browser.registered = true; browser.extension_ids = [args.extensionId as string]; return; }
          case "browser_setup_set_identities": { const browser = status.browsers.find((browser) => browser.browser === args.browser)!; browser.extension_ids = args.extensionIds as string[]; browser.registered = browser.extension_ids.length > 0; return; }
          case "browser_setup_unregister": status.browsers.find((browser) => browser.browser === args.browser)!.registered = false; return;
          case "browser_setup_open_store": case "browser_setup_open_files": return;
          case "capture_enable": status.capture_enabled = args.enabled as boolean; return;
          default: throw new Error(`Unstubbed setup fixture command: ${command}`);
        }
      },
    } });
  }, { helper, conflict, store, theme, id: ID });
  await page.goto("/");
  await page.getByRole("button", { name: "Capture & queues", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Capture & queues", exact: true });
  await dialog.getByRole("button", { name: "Set up browser extension", exact: true }).click();
  await expect(dialog.getByLabel("Browser to connect", { exact: true })).toBeFocused();
  await expect.poll(() => callsFor(page, "browser_setup_status")).not.toHaveLength(0);
  return dialog;
}
async function callsFor(page: Page, command: string) { return page.evaluate((command) => window.__browserSetup.calls.filter((call) => call.command === command), command); }

test("guided setup requires fresh per-browser consent, then observes a real native-handshake status", async ({ page }) => {
  const dialog = await bootstrap(page);
  await expect(dialog).toContainText("No published store listing is configured");
  await expect(dialog.getByRole("button", { name: "Open extension store", exact: true })).toHaveCount(0);
  const register = dialog.getByRole("button", { name: "Register browser helper", exact: true });
  await expect(register).toBeDisabled();
  await expect(dialog.getByLabel("Installed extension ID", { exact: true })).toHaveCount(0);
  const consent = dialog.getByRole("checkbox", { name: /I authorize OSDM to register/ });
  await consent.check();
  await dialog.getByLabel("Browser to connect", { exact: true }).selectOption("edge");
  await expect(consent).not.toBeChecked(); await expect(register).toBeDisabled();
  await dialog.getByLabel("Browser to connect", { exact: true }).selectOption("chrome");
  await consent.check();
  await register.click();
  await expect.poll(() => callsFor(page, "browser_setup_register")).toEqual([{ command: "browser_setup_register", args: { browser: "chrome", extensionId: ID, confirmed: true } }]);
  await expect(dialog).toContainText("Browser helper registered.");
  await expect(dialog).toContainText("No successful native-helper check received this session yet.");
  await dialog.getByRole("button", { name: "Enable local integration", exact: true }).click();
  await expect.poll(() => callsFor(page, "capture_enable")).toContainEqual({ command: "capture_enable", args: { enabled: true } });
  await expect(dialog.getByRole("checkbox", { name: "Enable local browser integration", exact: true })).toBeChecked();
  await page.evaluate(() => window.__browserSetup.handshake());
  await dialog.getByRole("button", { name: "Refresh setup status", exact: true }).click();
  await expect(dialog).toContainText("Last successful native-helper check this session:");
  const remove = dialog.getByRole("button", { name: "Remove helper registration", exact: true });
  await expect(remove).toBeDisabled();
  await dialog.getByRole("checkbox", { name: /I authorize removing only OSDM's helper/ }).check();
  await remove.click();
  await expect.poll(() => callsFor(page, "browser_setup_unregister")).toEqual([{ command: "browser_setup_unregister", args: { browser: "chrome", confirmed: true } }]);
});

test("configured store action delegates a browser enum, not an arbitrary URL or silent install", async ({ page }) => {
  const dialog = await bootstrap(page, { store: true });
  await expect(dialog.getByLabel("Installed extension ID", { exact: true })).toHaveCount(0);
  await expect(dialog).toContainText("no ID copying is needed");
  await dialog.getByRole("button", { name: "Open extension store", exact: true }).click();
  await expect.poll(() => callsFor(page, "browser_setup_open_store")).toEqual([{ command: "browser_setup_open_store", args: { browser: "chrome" } }]);
  expect(await callsFor(page, "browser_setup_register")).toEqual([]);
  await dialog.getByText("Advanced: load extension files manually", { exact: true }).click();
  await dialog.getByRole("button", { name: "Open extension files", exact: true }).click();
  await expect.poll(() => callsFor(page, "browser_setup_open_files")).toEqual([{ command: "browser_setup_open_files", args: { browser: "chrome" } }]);
});

for (const scenario of ["missing helper", "foreign registration"]) {
  test(`${scenario} blocks registration without changing the system`, async ({ page }) => {
    const dialog = await bootstrap(page, { helper: scenario !== "missing helper", conflict: scenario === "foreign registration" });
    await dialog.getByRole("checkbox", { name: /I authorize OSDM to register/ }).check();
    await expect(dialog.getByRole("button", { name: "Register browser helper", exact: true })).toBeDisabled();
    expect(await callsFor(page, "browser_setup_register")).toEqual([]);
    await expect(dialog).toContainText(scenario === "missing helper" ? "This build has no packaged native helper" : "different native-host registration");
  });
}

for (const theme of ["dark", "light"]) {
  test(`${theme} browser setup is accessible and closing restores its trigger`, async ({ page }) => {
    const dialog = await bootstrap(page, { theme });
    const scan = await new AxeBuilder({ page }).include('[role="dialog"]').analyze();
    expect(scan.violations).toEqual([]);
    const toggle = dialog.getByRole("button", { name: "Set up browser extension", exact: true });
    await toggle.click(); await expect(toggle).toBeFocused();
    await expect(dialog.getByLabel("Browser to connect", { exact: true })).toHaveCount(0);
  });
}

test("browser preview explains that it cannot inspect or register the helper", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("osdm.settings", JSON.stringify({ state: { settings: { onboarding_complete: true } }, version: 0 })));
  await page.goto("/");
  await page.getByRole("button", { name: "Capture & queues", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Capture & queues", exact: true });
  await dialog.getByRole("button", { name: "Set up browser extension", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText("Guided browser setup requires the desktop app");
  await expect(dialog.getByRole("button", { name: "Register browser helper", exact: true })).toBeDisabled();
});

test("Brave connects with the stable packaged identity without copying a profile ID", async ({ page }) => {
  const dialog = await bootstrap(page);
  await dialog.getByLabel("Browser to connect", { exact: true }).selectOption("brave");
  await expect(dialog.getByLabel("Installed extension ID", { exact: true })).toHaveCount(0);
  await dialog.getByRole("checkbox", { name: /I authorize OSDM to register/ }).check();
  await dialog.getByRole("button", { name: "Register browser helper", exact: true }).click();
  await expect.poll(() => callsFor(page, "browser_setup_register")).toEqual([{ command: "browser_setup_register", args: { browser: "brave", extensionId: ID, confirmed: true } }]);
});

test("multiple installations of one browser have add/edit/remove IDs with fresh replacement consent", async ({ page }) => {
  const dialog = await bootstrap(page);
  await dialog.getByRole("button", { name: "Manage extension installations" }).click();
  await dialog.getByRole("button", { name: "+ Add extension installation", exact: true }).click();
  await dialog.getByLabel("Extension ID 1", { exact: true }).fill(ID);
  const consent = dialog.getByRole("checkbox", { name: /I authorize replacing this browser/ });
  await consent.check();
  const save = dialog.getByRole("button", { name: "Save extension installations" });
  await expect(save).toBeEnabled();
  await dialog.getByRole("button", { name: "+ Add extension installation", exact: true }).click();
  await expect(consent).not.toBeChecked();
  await dialog.getByLabel("Extension ID 2", { exact: true }).fill("p".repeat(32));
  await consent.check(); await save.click();
  await expect.poll(() => callsFor(page, "browser_setup_set_identities")).toEqual([{ command: "browser_setup_set_identities", args: { browser: "chrome", extensionIds: [ID, "p".repeat(32)], confirmed: true } }]);
  await dialog.getByRole("button", { name: "Manage extension installations" }).click();
  await dialog.getByLabel("Extension ID 1", { exact: true }).fill("b".repeat(32));
  await dialog.getByRole("button", { name: "Remove extension installation 2" }).click();
  await consent.check(); await save.click();
  await expect.poll(() => callsFor(page, "browser_setup_set_identities")).toHaveLength(2);
});
