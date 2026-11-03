import { expect, test } from "@playwright/test";

test("emoji picker searches, picks, remembers and jumps to categories", async ({
  page,
}, testInfo) => {
  await page.goto("/tests/browser/emoji-picker.html");
  const picker = page.locator("[data-emoji-picker]");
  await expect(picker.getByRole("gridcell").first()).toBeVisible();
  await expect(picker.getByRole("searchbox")).toBeFocused();
  await page.screenshot({ path: testInfo.outputPath("open.png") });

  await picker.getByRole("searchbox").fill("rocket");
  await expect(picker.getByRole("gridcell", { name: "Rocket" })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("search.png") });
  await picker.getByRole("gridcell", { name: "Rocket" }).click();
  await expect(page.locator("[data-picked]")).toHaveText("🚀");
  await expect(picker).toHaveCount(0);

  // Reopened, the pick leads the recents and the flags are a jump away.
  await page.getByRole("button", { name: "Open" }).click();
  await expect(picker.getByRole("button", { name: "🚀" })).toBeVisible();
  await picker.getByRole("button", { name: "Jump to category 9" }).click();
  await expect(picker.getByText("Flags", { exact: true })).toBeInViewport();
  await page.screenshot({ path: testInfo.outputPath("flags.png") });

  await page.keyboard.press("Escape");
  await expect(picker).toHaveCount(0);
});
