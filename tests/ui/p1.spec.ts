import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() =>
    localStorage.setItem(
      "osdm.settings",
      JSON.stringify({
        state: {
          settings: {
            onboarding_complete: true,
            desktop: { silent: true },
          },
        },
        version: 0,
      }),
    ),
  );
  await page.goto("/");
  await expect(
    page
      .getByRole("banner")
      .getByRole("button", { name: "Add download", exact: true }),
  ).toBeEnabled();
});

test("defer, persist properties, filter category, customize columns and toolbar", async ({
  page,
}) => {
  await page.keyboard.press("n");
  await page
    .getByLabel("Source URL", { exact: true })
    .fill("https://example.test/p1.zip");
  await page.getByRole("button", { name: "Continue", exact: true }).click();
  await page.getByLabel("Description", { exact: true }).fill("P1 regression");
  await expect(page.getByLabel("Category", { exact: true })).toHaveValue(
    "compressed",
  );
  await page
    .getByRole("button", { name: "Download later", exact: true })
    .click();
  const row = page.locator("li[data-row]").filter({ hasText: "p1.zip" });
  await expect(row).toContainText("Paused");
  await page
    .getByRole("button", { name: "Category: Compressed", exact: true })
    .click();
  await expect(row).toBeVisible();
  await row.click();
  await expect(page.getByLabel("Description", { exact: true })).toHaveValue(
    "P1 regression",
  );
  await page
    .getByLabel("Description", { exact: true })
    .fill("Edited description");
  await page
    .getByRole("button", { name: "Save properties", exact: true })
    .click();
  await expect(
    page.getByText("Properties saved.", { exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Close inspector", exact: true })
    .click();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("checkbox", { name: "rate", exact: true }).uncheck();
  await page.getByRole("checkbox", { name: "pause", exact: true }).uncheck();
  await page.getByRole("button", { name: "Done", exact: true }).click();
  await expect(page.getByRole("button", { name: "Sort by Rate" })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole("button", { name: "Pause active", exact: true }),
  ).toHaveCount(0);
  await row.getByRole("checkbox").check();
  await expect(
    page.getByRole("button", { name: "Delete selected", exact: true }),
  ).toBeDisabled();
});

for (const theme of ["dark", "light"] as const) {
  test(`${theme} list and settings accessibility`, async ({ page }) => {
    await page.goto("/?demo");
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.evaluate(
      (theme) => (document.documentElement.dataset.theme = theme),
      theme,
    );
    const list = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    expect(list.violations).toEqual([]);
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    const settings = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    expect(settings.violations).toEqual([]);
  });
}

test("keyboard add dialog restores focus and scaling keeps controls available", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page
    .getByRole("banner")
    .getByRole("button", { name: "Add download", exact: true })
    .focus();
  await page.keyboard.press("Enter");
  await expect(page.getByLabel("Source URL", { exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(
    page
      .getByRole("banner")
      .getByRole("button", { name: "Add download", exact: true }),
  ).toBeFocused();
  await page.setViewportSize({ width: 800, height: 600 });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "200%";
  });
  await expect(
    page.getByRole("button", { name: "Done", exact: true }),
  ).toBeInViewport();
});

test("onboarding can check or skip without queueing a download", async ({
  page,
}) => {
  await page.evaluate(() => localStorage.removeItem("osdm.settings"));
  const onboarding = await page.context().newPage();
  await onboarding.goto("/");
  await expect(
    onboarding.getByRole("dialog", { name: "Welcome to OSDM" }),
  ).toBeVisible();
  await onboarding
    .getByRole("button", { name: "Skip connection check", exact: true })
    .click();
  await expect(onboarding.getByRole("dialog")).toHaveCount(0);
  await expect(onboarding.locator("li[data-row]")).toHaveCount(0);
});
