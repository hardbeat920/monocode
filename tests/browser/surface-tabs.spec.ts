import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() =>
    localStorage.setItem("monocode.tabAnimationsEnabled", "true"),
  );
  await page.goto("/tests/browser/surface-tabs.html");
});

test("file tabs shrink before scrolling and retain their gap", async ({
  page,
}) => {
  const tabs = page.locator("[data-tab-slot-id]");
  const strip = page.getByRole("tablist");
  const widths = await tabs.evaluateAll((els) =>
    els.map((el) => el.getBoundingClientRect().width),
  );
  for (const width of widths) {
    expect(width).toBeGreaterThanOrEqual(112);
    expect(width).toBeLessThan(224);
  }
  expect(await strip.evaluate((el) => el.scrollWidth - el.clientWidth)).toBe(0);
  expect(await strip.evaluate((el) => getComputedStyle(el).columnGap)).toBe(
    "2px",
  );
  await page.getByRole("button", { name: "Expand", exact: true }).click();
  await expect
    .poll(() => tabs.first().evaluate((el) => el.getBoundingClientRect().width))
    .toBe(224);
  await page.getByRole("button", { name: "Narrow", exact: true }).click();
  await expect
    .poll(() => tabs.first().evaluate((el) => el.getBoundingClientRect().width))
    .toBe(112);
  expect(
    await strip.evaluate((el) => el.scrollWidth - el.clientWidth),
  ).toBeGreaterThan(0);
});

test("active tab reveal scrolls only the file row", async ({ page }) => {
  await page.getByRole("button", { name: "Narrow", exact: true }).click();
  const ancestor = page.locator("[data-ancestor]");
  await ancestor.evaluate((el) => {
    el.scrollTop = 16;
  });
  await page.getByRole("button", { name: "Select last", exact: true }).click();
  const strip = page.getByRole("tablist");
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
  expect(await ancestor.evaluate((el) => el.scrollTop)).toBe(16);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
});

test("opening and closing keep motion widths and reveal the new active tab", async ({
  page,
}) => {
  await page.getByRole("button", { name: "Narrow", exact: true }).click();
  await page.getByRole("button", { name: "Open file", exact: true }).click();
  await expect(page.locator("[data-opening-tab]")).toHaveCount(1);
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
  await page.getByRole("button", { name: "Close d.tsx", exact: true }).click();
  const closing = page.locator("[data-closing-tab]");
  await expect(closing).toHaveCount(1);
  expect(
    await closing.evaluate((el) =>
      el.style.getPropertyValue("--tab-slot-width"),
    ),
  ).toBe("112px");
  await expect(closing).toHaveCount(0);
  await expect(page.locator("[data-tab-slot-id]")).toHaveCount(3);
});

test("drag reorder stays stable while the file row is scrolled", async ({
  page,
}) => {
  await page.getByRole("button", { name: "Narrow", exact: true }).click();
  await page.getByRole("button", { name: "Select last", exact: true }).click();
  const strip = page.getByRole("tablist");
  const from = (await page.locator('[data-tab-slot-id="c"]').boundingBox())!;
  const to = (await page.locator('[data-tab-slot-id="b"]').boundingBox())!;
  const before = await strip.evaluate((el) => el.scrollLeft);
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, {
    steps: 12,
  });
  await expect(page.locator("[data-dragging]")).toHaveCount(1);
  expect(await strip.evaluate((el) => el.scrollLeft)).toBe(before);
  await page.mouse.up();
  await expect(page.locator("[data-order]")).toHaveText("acb");
  await expect(page.locator("[data-reordering]")).toHaveCount(0);
});
