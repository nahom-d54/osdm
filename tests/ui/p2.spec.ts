import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

test.beforeEach(async ({ page }, testInfo) => {
  const theme = testInfo.title.startsWith("light ") ? "light" : "dark";
  await page.emulateMedia({ reducedMotion: "reduce", colorScheme: theme });
  // Start with the persisted theme instead of changing CSS variables while axe
  // scans an opening dialog's color transitions.
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
  await page
    .getByRole("button", { name: "Capture & queues", exact: true })
    .click();
});

test("batch admission, duplicate reuse, queue navigation and transfer windows", async ({
  page,
}) => {
  const dialog = page.getByRole("dialog", {
    name: "Capture & queues",
    exact: true,
  });
  await page.getByLabel("Queue name", { exact: true }).fill("Night shift");
  await page.getByRole("button", { name: "Create queue", exact: true }).click();
  await expect(page.getByLabel("Batch queue", { exact: true })).not.toHaveValue(
    "",
  );
  await page
    .getByLabel("Batch links", { exact: true })
    .fill(
      "https://example.test/first.zip\nhttps://example.test/second.zip\nhttps://example.test/first.zip#again",
    );
  await page
    .getByRole("button", { name: "Save for later", exact: true })
    .click();
  await expect(dialog.getByRole("status")).toContainText(
    "2 saved · 0 existing · 0 failed",
  );
  await page
    .getByRole("button", { name: "Save for later", exact: true })
    .click();
  await expect(dialog.getByRole("status")).toContainText(
    "2 saved · 2 existing · 0 failed",
  );
  await page
    .getByLabel("Schedule first start", { exact: true })
    .fill("2099-01-01T22:00");
  await page.getByLabel("Schedule repeat hours", { exact: true }).fill("24");
  await page.getByLabel("Schedule window minutes", { exact: true }).fill("120");
  await page
    .getByRole("button", { name: "Save schedule", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Delete schedule", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await page
    .getByRole("button", { name: "Queue: Night shift", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Night shift", exact: true }),
  ).toBeVisible();
  await expect(page.locator("li[data-row]")).toHaveCount(2);
  await expect(page.locator("li[data-row]").first()).toContainText("Paused");
  await page
    .getByRole("button", { name: "Capture & queues", exact: true })
    .click();
  await page.getByRole("button", { name: "Start queue", exact: true }).click();
  await page.getByRole("button", { name: "Stop queue", exact: true }).click();
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await expect(page.locator("li[data-row]").first()).toContainText("Paused");
  await page
    .getByRole("button", { name: "Capture & queues", exact: true })
    .click();
  await page.getByRole("button", { name: "Delete", exact: true }).click();
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Queue: Night shift", exact: true }),
  ).toHaveCount(0);
  await expect(page.locator("li[data-row]")).toHaveCount(2); // Deleting the selected queue never hides or removes its downloads.
});

test("unsafe input does not admit entries and native-only actions stay explicit", async ({
  page,
}) => {
  await page
    .getByLabel("Batch links", { exact: true })
    .fill("https://example.test/ordinary.zip\nhttps://example.test/a%2fb.zip");
  await page
    .getByRole("button", { name: "Save for later", exact: true })
    .click();
  await expect(
    page.getByText(
      "A link has no safe filename. Add that link individually and choose its filename.",
    ),
  ).toBeVisible();
  await expect(
    page.getByRole("checkbox", {
      name: "Enable local browser integration",
      exact: true,
    }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Arm completion action", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await expect(page.locator("li[data-row]")).toHaveCount(0);
});

for (const theme of ["dark", "light"] as const) {
  test(`${theme} capture dialog accessibility and keyboard close`, async ({
    page,
  }) => {
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    expect(results.violations).toEqual([]);
    await page.keyboard.press("Escape");
    await expect(
      page.getByRole("button", { name: "Capture & queues", exact: true }),
    ).toBeFocused();
  });
}
