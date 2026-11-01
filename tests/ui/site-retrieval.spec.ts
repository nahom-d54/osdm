import { test, expect, type Locator, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

test.beforeEach(async ({ page }, testInfo) => {
  const theme = testInfo.title.startsWith("light ") ? "light" : "dark";
  await page.emulateMedia({ reducedMotion: "reduce", colorScheme: theme });
  await page.addInitScript((theme) => localStorage.setItem("osdm.settings", JSON.stringify({
    state: { settings: { theme, onboarding_complete: true, desktop: { silent: true } } }, version: 0,
  })), theme);
  await page.goto("/");
});

async function openSite(page: Page) {
  await page.getByRole("button", { name: "Site Grabber", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Site Grabber", exact: true });
  await expect(dialog).toBeVisible();
  return dialog;
}

async function sourceStep(dialog: Locator) {
  await dialog.getByLabel("Project name", { exact: true }).fill("Fixture manual");
  await dialog.getByLabel("Starting page URL", { exact: true }).fill("https://site.example.test/manual/index.html");
  await dialog.getByLabel("Output folder", { exact: true }).fill("C:\\FakeDownloads\\manual");
  await dialog.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(dialog.getByLabel("Allowed directory path", { exact: true })).toHaveValue("/manual/");
}

async function budgetStep(dialog: Locator) {
  await sourceStep(dialog);
  await dialog.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(dialog.getByLabel("Allowed file extensions", { exact: true })).toBeVisible();
}

async function noInventedResults(dialog: Locator, page: Page) {
  await expect(dialog.getByRole("heading", { name: "Saved projects", exact: true })).toHaveCount(0);
  await expect(dialog.getByRole("group", { name: /Review resources/ })).toHaveCount(0);
  await expect(dialog.getByRole("progressbar")).toHaveCount(0);
  await expect(dialog).not.toContainText(/\d+ pages ·|\d+ resources added|\d+(?:\.\d+)?\s*%|Loading the saved project/);
  await expect(page.locator("li[data-row]")).toHaveCount(0);
}

test("wizard keyboard focus follows newly displayed steps", async ({ page }) => {
  const opener = page.getByRole("button", { name: "Site Grabber", exact: true });
  await opener.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Site Grabber", exact: true });
  await expect(dialog.getByLabel("Project name", { exact: true })).toBeFocused();
  await sourceStep(dialog);
  await expect(dialog.locator('[aria-current="step"]')).toContainText("Scope");
  await expect.soft(dialog.getByLabel("Allowed directory path", { exact: true })).toBeFocused();
  await dialog.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(dialog.locator('[aria-current="step"]')).toContainText("Filters & budgets");
  await expect.configure({ soft: true }).poll(() => dialog.locator('[data-slot="dialog-body"]').evaluate((node) => node.contains(document.activeElement))).toBe(true);
  await dialog.getByRole("button", { name: "Back", exact: true }).click();
  await expect.soft(dialog.getByLabel("Allowed directory path", { exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(opener).toBeFocused();
});

test("keyboard and footer closing restore focus without inventing saved projects", async ({ page }) => {
  const opener = page.getByRole("button", { name: "Site Grabber", exact: true });
  await opener.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Site Grabber", exact: true });
  await expect(dialog.getByLabel("Project name", { exact: true })).toBeFocused();
  await noInventedResults(dialog, page);
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(opener).toBeFocused();
  await page.keyboard.press("Space");
  await expect(dialog).toBeVisible();
  await dialog.locator('[data-slot="dialog-footer"]').getByRole("button", { name: "Close", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(opener).toBeFocused();
});

test("finite budgets and explicit confirmation gate exploration; browser preview never crawls", async ({ page }) => {
  const requests: string[] = [];
  await page.route("https://site.example.test/**", async (route) => {
    requests.push(route.request().url());
    await route.abort();
  });
  const dialog = await openSite(page);
  await noInventedResults(dialog, page);
  await budgetStep(dialog);
  const save = dialog.getByRole("button", { name: "Save project & explore", exact: true });
  const consent = dialog.getByRole("checkbox", { name: /I authorize anonymous exploration only/ });
  await expect(consent).not.toBeChecked();
  await expect(save).toBeDisabled();
  await dialog.getByRole("button", { name: "Images", exact: true }).click();
  await expect(dialog.getByLabel("Allowed file extensions", { exact: true })).toHaveValue("png jpg jpeg gif webp svg");
  await consent.check();
  await dialog.getByLabel("Pages (1–1,000)", { exact: true }).fill("0");
  await save.click();
  await expect(dialog.getByRole("alert")).toContainText("Choose finite budgets within the displayed limits.");
  await dialog.getByLabel("Pages (1–1,000)", { exact: true }).fill("20");
  await dialog.getByLabel("Files (1–10,000)", { exact: true }).fill("1.5");
  await save.click();
  await expect(dialog.getByRole("alert")).toContainText("Budgets must be whole numbers.");
  await dialog.getByLabel("Files (1–10,000)", { exact: true }).fill("100");
  await save.click();
  await expect(dialog.getByRole("alert")).toContainText("Site exploration requires the desktop app. Browser preview does not crawl websites.");
  await expect(dialog.locator('[aria-current="step"]')).toContainText("Filters & budgets");
  await noInventedResults(dialog, page);
  expect(requests).toEqual([]);
  await dialog.getByRole("button", { name: "Back", exact: true }).click();
  await dialog.getByLabel("Allowed directory path", { exact: true }).fill("/");
  await dialog.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(consent).not.toBeChecked();
  await expect(save).toBeDisabled();
  await dialog.getByRole("button", { name: "Back", exact: true }).click();
  await dialog.getByRole("button", { name: "Back", exact: true }).click();
  await noInventedResults(dialog, page);
});

test("scope cannot expand to an unrelated directory", async ({ page }) => {
  const dialog = await openSite(page);
  await sourceStep(dialog);
  await dialog.getByLabel("Allowed directory path", { exact: true }).fill("/private/");
  await dialog.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText("Use an absolute directory path ending in / that contains the seed page.");
  await expect(dialog.locator('[aria-current="step"]')).toContainText("Scope");
  await noInventedResults(dialog, page);
});

test("project templates apply bounded filters without authorizing a source or changing its destination", async ({ page }) => {
  const dialog = await openSite(page);
  await dialog.getByLabel("Project template", { exact: true }).selectOption("audio");
  await budgetStep(dialog);
  await expect(dialog.getByLabel("Allowed file extensions", { exact: true })).toHaveValue("mp3 m4a ogg opus flac wav");
  await expect(dialog.getByLabel("Pages (1–1,000)", { exact: true })).toHaveValue("10");
  const consent = dialog.getByRole("checkbox", { name: /I authorize anonymous exploration only/ });
  await expect(consent).not.toBeChecked();
  await consent.check();
  await dialog.getByRole("button", { name: "Back", exact: true }).click();
  await dialog.getByRole("button", { name: "Back", exact: true }).click();
  await dialog.getByLabel("Project template", { exact: true }).selectOption("documents");
  await expect(dialog.getByLabel("Starting page URL", { exact: true })).toHaveValue("https://site.example.test/manual/index.html");
  await expect(dialog.getByLabel("Output folder", { exact: true })).toHaveValue("C:\\FakeDownloads\\manual");
  await dialog.getByRole("button", { name: "Continue", exact: true }).click();
  await dialog.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(consent).not.toBeChecked();
  await expect(dialog.getByRole("button", { name: "Save project & explore", exact: true })).toBeDisabled();
});

for (const theme of ["dark", "light"] as const) {
  test(`${theme} site wizard source and budget steps are accessible`, async ({ page }) => {
    const dialog = await openSite(page);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const source = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
    expect(source.violations).toEqual([]);
    await budgetStep(dialog);
    const budgets = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
    expect(budgets.violations).toEqual([]);
  });
}
