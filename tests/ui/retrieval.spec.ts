import { test, expect, type Locator, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

test.beforeEach(async ({ page }, testInfo) => {
  const theme = testInfo.title.startsWith("light ") ? "light" : "dark";
  await page.emulateMedia({ reducedMotion: "reduce", colorScheme: theme });
  await page.addInitScript(
    (theme) =>
      localStorage.setItem(
        "osdm.settings",
        JSON.stringify({
          state: {
            settings: {
              theme,
              onboarding_complete: true,
              desktop: { silent: true },
            },
          },
          version: 0,
        }),
      ),
    theme,
  );
  await page.goto("/");
  await expect(
    page.getByRole("button", { name: "Save media", exact: true }),
  ).toBeEnabled();
});

async function openMedia(page: Page) {
  await page.getByRole("button", { name: "Save media", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Save media", exact: true });
  await expect(dialog).toBeVisible();
  return dialog;
}

async function expectNoInventedMedia(dialog: Locator, page: Page) {
  await expect(
    dialog.getByRole("group", { name: "Choose a quality", exact: true }),
  ).toHaveCount(0);
  await expect(dialog.locator('input[name="media-quality"]')).toHaveCount(0);
  await expect(dialog.getByRole("progressbar")).toHaveCount(0);
  await expect(
    dialog.getByRole("region", { name: "Saved media jobs", exact: true }),
  ).toHaveCount(0);
  await expect(dialog).not.toContainText(/\d+\s*(?:×|x)\s*\d+|\d+\s*kb\/s declared|\d+\s*\/\s*\d+ segments complete|\d+ ordered segments|\d+(?:\.\d+)?\s*%/);
  await expect(
    dialog.getByRole("button", { name: "Save selected media", exact: true }),
  ).toBeDisabled();
  await expect(page.locator("li[data-row]")).toHaveCount(0);
}

test("keyboard opens and closes media dialog, traps focus, and restores its opener", async ({ page }) => {
  const opener = page.getByRole("button", { name: "Save media", exact: true });
  await opener.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Save media", exact: true });
  await expect(dialog).toBeVisible();
  await expect.poll(() => dialog.evaluate((node) => node.contains(document.activeElement))).toBe(true);
  const close = dialog.locator('[data-slot="dialog-close"]');
  await close.focus();
  await page.keyboard.press("Tab");
  await expect.poll(() => dialog.evaluate((node) => node.contains(document.activeElement))).toBe(true);
  await page.keyboard.press("Shift+Tab");
  await expect(close).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(opener).toBeFocused();

  await page.keyboard.press("Space");
  await expect(dialog).toBeVisible();
  await dialog.locator('[data-slot="dialog-footer"]').getByRole("button", { name: "Close", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(opener).toBeFocused();
});

test("browser inspection requires desktop without fetching or fabricating quality, jobs, or progress", async ({ page }) => {
  const requests: string[] = [];
  // These are synthetic test origins, not captured or signed media URLs.
  await page.route("https://media.example.test/**", async (route) => {
    requests.push(route.request().url());
    await route.abort();
  });
  const dialog = await openMedia(page);
  await expectNoInventedMedia(dialog, page);
  await dialog.getByLabel("Media URL", { exact: true }).fill("https://media.example.test/master.m3u8");
  await dialog.getByRole("button", { name: "Inspect manifest", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText("Media inspection requires the desktop app. Browser preview does not fetch media.");
  await expect(dialog.getByRole("button", { name: "Inspect manifest", exact: true })).toBeEnabled();
  await expectNoInventedMedia(dialog, page);
  expect(requests).toEqual([]);
  await dialog.locator('[data-slot="dialog-footer"]').getByRole("button", { name: "Close", exact: true }).click();
  const reopened = await openMedia(page);
  await expect(reopened.getByLabel("Media URL", { exact: true })).toHaveValue("");
  await expect(reopened.getByRole("alert")).toHaveCount(0);
  await expectNoInventedMedia(reopened, page);
});

for (const [name, origin] of [
  ["non-HTTP scheme", "file:///media"],
  ["credentials", "https://user:password@cdn.example.test"],
  ["path", "https://cdn.example.test/media"],
  ["query", "https://cdn.example.test?token=fixture"],
  ["fragment", "https://cdn.example.test#fragment"],
  ["trailing slash", "https://cdn.example.test/"],
] as const) {
  test(`unsafe approved origin rejects ${name} without admitting media`, async ({ page }) => {
    const dialog = await openMedia(page);
    await dialog.getByLabel("Media URL", { exact: true }).fill("https://media.example.test/master.m3u8");
    await dialog.getByLabel("Additional approved origins", { exact: true }).fill(origin);
    await dialog.getByRole("button", { name: "Inspect manifest", exact: true }).click();
    await expect(dialog.getByRole("alert")).toContainText("Enter HTTP(S) origins, without paths, credentials, queries, or a trailing slash.");
    await expect(dialog.getByRole("alert")).not.toContainText("requires the desktop app");
    await expectNoInventedMedia(dialog, page);
  });
}

test("narrow media dialog stays within viewport without horizontal overflow", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const dialog = await openMedia(page);
  await dialog.getByLabel("Media URL", { exact: true }).fill(`https://media.example.test/${"long-filename-".repeat(20)}master.m3u8`);
  await dialog.getByLabel("Additional approved origins", { exact: true }).fill("https://cdn.example.test");
  await expect(dialog.locator('[data-slot="dialog-footer"]').getByRole("button", { name: "Close", exact: true })).toBeInViewport();
  await expect(dialog.getByRole("button", { name: "Save selected media", exact: true })).toBeInViewport();
  await expect.poll(() => dialog.evaluate((node) => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
  const bounds = await dialog.boundingBox();
  expect(bounds).not.toBeNull();
  expect(bounds!.x).toBeGreaterThanOrEqual(-1);
  expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(391);
  const screenshot = testInfo.outputPath("media-dialog-narrow.png");
  await page.screenshot({ path: screenshot });
  await testInfo.attach("media-dialog-narrow", {
    path: screenshot,
    contentType: "image/png",
  });
});

for (const theme of ["dark", "light"] as const) {
  test(`${theme} media dialog accessibility before and after desktop-only inspection error`, async ({ page }) => {
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const dialog = await openMedia(page);
    const initial = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    expect(initial.violations).toEqual([]);
    await dialog.getByLabel("Media URL", { exact: true }).fill("https://media.example.test/master.m3u8");
    await dialog.getByRole("button", { name: "Inspect manifest", exact: true }).click();
    await expect(dialog.getByRole("alert")).toContainText("requires the desktop app");
    const error = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    expect(error.violations).toEqual([]);
  });
}
