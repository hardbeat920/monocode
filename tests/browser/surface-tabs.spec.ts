import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() =>
    localStorage.setItem("monocode.tabAnimationsEnabled", "true"),
  );
  await page.goto("/tests/browser/surface-tabs.html");
});

test("active tab reveal scrolls only the file row", async ({ page }) => {
  const ancestor = page.locator("[data-ancestor]");
  await ancestor.evaluate((el) => {
    el.scrollTop = 0;
  });
  const strip = page.getByRole("tablist");
  expect(
    await strip.evaluate((el) => el.scrollWidth - el.clientWidth),
  ).toBeGreaterThan(0);
  expect(
    await ancestor.evaluate(
      (el) =>
        el.querySelector('[role="tablist"]')!.getBoundingClientRect().bottom -
        el.getBoundingClientRect().bottom,
    ),
  ).toBeGreaterThan(0);
  await page.getByRole("button", { name: "Select last", exact: true }).click();
  const selected = page.locator('[data-tab-slot-id="c"]');
  await expect
    .poll(async () => {
      const tab = (await selected.boundingBox())!;
      const viewport = (await strip.boundingBox())!;
      return (
        tab.x >= viewport.x - 1 &&
        tab.x + tab.width <= viewport.x + viewport.width + 1
      );
    })
    .toBe(true);
  expect(await ancestor.evaluate((el) => el.scrollTop)).toBe(0);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
});

test("opening motion reveals the new active tab", async ({ page }) => {
  await page.getByRole("button", { name: "Open file", exact: true }).click();
  await expect(page.locator("[data-opening-tab]")).toHaveCount(0);
  const tab = page.locator('[data-tab-slot-id="d"]');
  await expect
    .poll(() => tab.evaluate((el) => el.getBoundingClientRect().width))
    .toBe(112);
  const viewport = (await page.getByRole("tablist").boundingBox())!;
  const box = (await tab.boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(viewport.x - 1);
  expect(box.x + box.width).toBeLessThanOrEqual(
    viewport.x + viewport.width + 1,
  );
});
