import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("monocode.newLayoutPromptDismissed", "1");
  });
});

test("focusing a split pane by keyboard moves typing into its composer", async ({
  page,
}) => {
  test.setTimeout(60_000);
  await page.goto("/tests/browser/workspace-performance.html");
  const left = page.locator('[data-session-drop="workspace-session-0"]');
  await expect(left).toBeVisible({ timeout: 60_000 });

  await page.keyboard.press("Meta+d");
  const right = page.locator(
    '[data-session-drop]:not([data-session-drop^="workspace-session-"])',
  );
  await expect(right).toBeVisible();
  const leftInput = left.locator("[data-composer] textarea");
  const rightInput = right.locator("[data-composer] textarea");

  await leftInput.click();
  await page.keyboard.type("left");

  await page.keyboard.press("Meta+Alt+ArrowRight");
  await expect(rightInput).toBeFocused();
  await page.keyboard.type("right");

  await page.keyboard.press("Meta+Alt+ArrowLeft");
  await expect(leftInput).toBeFocused();

  await expect(leftInput).toHaveValue("left");
  await expect(rightInput).toHaveValue("right");
});
