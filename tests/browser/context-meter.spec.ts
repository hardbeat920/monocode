import { expect, test, type Page } from "@playwright/test";

const COMPACTION = { used: 234_000, window: 1_000_000, compactionWindow: 400_000 };
const LINES = [
  "59% of auto-compaction window",
  "234K / 400K tokens",
  "Full model capacity: 1M tokens",
];

type Usage = { used: number; window?: number; compactionWindow?: number };

async function setUsage(page: Page, usage: Usage | undefined) {
  await page.evaluate((value) => window.setContextUsage(value), usage);
}

async function setTheme(page: Page, theme: "light" | "dark") {
  await page.evaluate((value) => window.setContextTheme(value), theme);
}

const meter = (page: Page) => page.getByRole("button", { name: /Open context actions/ });

async function tooltipLines(page: Page) {
  const host = page.getByText(LINES[0], { exact: true });
  await expect(host).toBeVisible();
  return host.locator("xpath=..").locator("> *").allTextContents();
}

async function ringState(page: Page) {
  return meter(page).locator("svg").evaluate((svg) => {
    const arc = svg.querySelectorAll("circle")[1];
    const color = svg.getAttribute("class") ?? "";
    if (!arc) return { ratio: 0, color };
    const radius = Number(arc.getAttribute("r"));
    const circumference = 2 * Math.PI * radius;
    const offset = Number(arc.getAttribute("stroke-dashoffset"));
    return { ratio: 1 - offset / circumference, color };
  });
}

test.beforeEach(async ({ page }) => {
  await page.goto("/tests/browser/context-meter.html");
  await expect(meter(page)).toBeVisible();
});

for (const theme of ["dark", "light"] as const) {
  test(`shows the compaction window reading on hover and click (${theme})`, async ({
    page,
  }, testInfo) => {
    await setTheme(page, theme);
    await meter(page).hover();
    expect(await tooltipLines(page)).toEqual(LINES);
    const ring = await ringState(page);
    expect(ring.ratio).toBeCloseTo(0.585, 3);
    await expect(meter(page)).toHaveAttribute(
      "aria-label",
      `${LINES.join(", ")}. Open context actions`,
    );

    const lineSizes = await page
      .getByText(LINES[1], { exact: true })
      .evaluate((node) => ({
        size: parseFloat(getComputedStyle(node).fontSize),
        numeric: getComputedStyle(node).fontVariantNumeric,
      }));
    expect(lineSizes.size).toBeGreaterThanOrEqual(12);
    expect(lineSizes.numeric).toContain("tabular-nums");

    await page.mouse.move(0, 0);
    await expect(page.getByText(LINES[0], { exact: true })).toHaveCount(0);
    await meter(page).click();
    expect(await tooltipLines(page)).toEqual([...LINES, "Compact now"]);
    await page.evaluate(() =>
      Promise.all(document.getAnimations().map((animation) => animation.finished)),
    );
    await page.screenshot({
      path: testInfo.outputPath(`context-meter-${theme}.png`),
      clip: { x: 760, y: 40, width: 240, height: 220 },
    });
  });
}

test("colours the ring against the display ratio", async ({ page }) => {
  await setUsage(page, { used: 300_000, window: 1_000_000, compactionWindow: 400_000 });
  expect((await ringState(page)).color).toContain("text-amber-400");
  await setUsage(page, { used: 370_000, window: 1_000_000, compactionWindow: 400_000 });
  expect((await ringState(page)).color).toContain("text-red-400");
  await setUsage(page, { used: 450_000, window: 1_000_000, compactionWindow: 400_000 });
  expect((await ringState(page)).ratio).toBeCloseTo(1, 5);
  await setUsage(page, COMPACTION);
  const calm = await ringState(page);
  expect(calm.color).not.toContain("text-amber-400");
  expect(calm.color).not.toContain("text-red-400");
});

test("falls back to the full-capacity reading without compaction metadata", async ({
  page,
}) => {
  await setUsage(page, { used: 234_000, window: 1_000_000 });
  await meter(page).hover();
  await expect(page.getByText("23% context used", { exact: true })).toBeVisible();
  await expect(page.getByText("234K / 1M tokens", { exact: true })).toBeVisible();
  await expect(page.getByText(/Full model capacity/)).toHaveCount(0);
  expect((await ringState(page)).ratio).toBeCloseTo(0.234, 3);
  await expect(meter(page)).toHaveAttribute(
    "aria-label",
    "23% context used, 234K / 1M tokens. Open context actions",
  );
});

test("keyboard opens the actions, Escape closes and Compact now still fires", async ({
  page,
}) => {
  await meter(page).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: /Compact/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByText(LINES[0], { exact: true })).toHaveCount(0);

  await meter(page).click();
  await page.getByRole("button", { name: "Compact now" }).click();
  expect(await page.evaluate(() => window.compactCount)).toBe(1);
  await expect(page.getByRole("button", { name: "Compact now" })).toHaveCount(0);
});

test("fits a 320px viewport without horizontal overflow", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 480 });
  await meter(page).click();
  await expect(page.getByText(LINES[2], { exact: true })).toBeVisible();
  const box = await page.getByText(LINES[2], { exact: true }).boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(320);
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
});
