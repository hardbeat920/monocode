import { expect, test, type Page } from "@playwright/test";

// Leave room for all five tabs beside Linux's title and window controls.
test.use({ viewport: { width: 1440, height: 800 } });

type Fixture = Window & {
  order: string[];
  placed: string[];
  startTicking: () => void;
};

const order = (page: Page) =>
  page.evaluate(() => (window as unknown as Fixture).order.join(""));

const placed = (page: Page) =>
  page.evaluate(() => (window as unknown as Fixture).placed);

async function center(page: Page, id: string) {
  const box = await page.locator(`[data-title-tab-id="${id}"]`).boundingBox();
  if (!box) throw new Error(`no tab ${id}`);
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

async function drag(
  page: Page,
  fromId: string,
  toId: string,
  { steps = 12, settle = 400, sag = 0 } = {},
) {
  const from = await center(page, fromId);
  const to = await center(page, toId);
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(to.x, to.y + sag, { steps });
  await page.mouse.up();
  if (settle) await page.waitForTimeout(settle);
}

test.beforeEach(async ({ page }) => {
  // Exercise the Linux CI window-controls path even when running on macOS.
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "platform", { value: "Linux x86_64" });
  });
  await page.goto("/tests/browser/title-tab-reorder.html");
  await expect(page.locator("[data-title-tab-id]")).toHaveCount(5);
});

test("slow drag reorders", async ({ page }) => {
  await drag(page, "a", "c", { steps: 30 });
  expect(await order(page)).toBe("bcade");
});

test("fast flick reorders", async ({ page }) => {
  await drag(page, "a", "d", { steps: 2 });
  expect(await order(page)).toBe("bcdae");
});

test("back-to-back drags both land", async ({ page }) => {
  await drag(page, "a", "c", { settle: 0 });
  await drag(page, "e", "a", { settle: 0 });
  await page.waitForTimeout(400);
  expect(await order(page)).toBe("ebcad");
});

test("drag while the title bar re-renders", async ({ page }) => {
  await page.evaluate(() => (window as unknown as Fixture).startTicking());
  await drag(page, "a", "c", { steps: 30 });
  expect(await order(page)).toBe("bcade");
});

test("an inactive tab that sags below the title bar still reorders", async ({
  page,
}) => {
  await drag(page, "b", "d", { steps: 20, sag: 25 });
  expect(await order(page)).toBe("acdbe");
  expect(await placed(page)).toEqual([]);
});

test("pulling an inactive tab well below the title bar places it on a pane", async ({
  page,
}) => {
  await drag(page, "b", "d", { steps: 20, sag: 120 });
  expect(await order(page)).toBe("abcde");
  expect(await placed(page)).toEqual(["b"]);
});

test("a fast release whose last moves report no button still drops", async ({
  page,
}) => {
  const from = await center(page, "a");
  const to = await center(page, "d");
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(to.x, to.y, { steps: 2 });
  // macOS WebKit stamps queued moves with the live button state, so a quick
  // release delivers them button-less just ahead of the pointerup.
  await page.evaluate(
    ({ x, y }) =>
      window.dispatchEvent(
        new PointerEvent("pointermove", {
          clientX: x,
          clientY: y,
          pointerId: 1,
          pointerType: "mouse",
          buttons: 0,
        }),
      ),
    to,
  );
  await page.mouse.up();
  await page.waitForTimeout(400);
  expect(await order(page)).toBe("bcdae");
});
