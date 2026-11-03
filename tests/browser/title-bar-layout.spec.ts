import { expect, test, type Page } from "@playwright/test";

test.use({ viewport: { width: 1440, height: 800 } });

const toggle = (page: Page) =>
  page.getByRole("button", { name: /^Toggle Sidebar/ });

async function left(page: Page, selector: string) {
  return (await page.locator(selector).boundingBox())!.x;
}

async function setFullscreen(page: Page, fullscreen: boolean) {
  await page.evaluate(async (next) => {
    await (
      window as unknown as {
        setFullscreen: (value: boolean) => Promise<void>;
      }
    ).setFullscreen(next);
  }, fullscreen);
}

for (const platform of ["MacIntel", "Win32", "Linux x86_64"]) {
  test.describe(platform, () => {
    test.beforeEach(async ({ page }) => {
      await page.addInitScript((value) => {
        Object.defineProperty(navigator, "platform", { value });
      }, platform);
    });

    test("fullscreen navigation keeps a small leading inset with the sidebar open or closed", async ({
      page,
    }) => {
      await page.goto("/tests/browser/title-tab-reorder.html?inset&fullscreen");
      await expect(toggle(page)).toBeVisible();
      await expect
        .poll(async () => (await toggle(page).boundingBox())!.x)
        .toBe(10);
      const closedTabs = await left(page, "[data-title-tab-strip]");
      expect(closedTabs).toBeLessThan(130);

      await toggle(page).click();
      await expect.poll(() => left(page, "[data-title-tab-strip]")).toBe(224);
      expect(await left(page, "[data-layout-card]")).toBe(224);
      expect((await toggle(page).boundingBox())!.x).toBe(10);

      await toggle(page).click();
      await expect
        .poll(() => left(page, "[data-title-tab-strip]"))
        .toBe(closedTabs);
    });

    test("updates the inset when entering and leaving native fullscreen", async ({
      page,
    }) => {
      await page.goto("/tests/browser/title-tab-reorder.html?inset&sidebar");
      const windowedInset = platform === "MacIntel" ? 78 : 10;
      await expect
        .poll(async () => (await toggle(page).boundingBox())!.x)
        .toBe(windowedInset);

      await setFullscreen(page, true);
      await expect
        .poll(async () => (await toggle(page).boundingBox())!.x)
        .toBe(10);
      expect(await left(page, "[data-title-tab-strip]")).toBe(224);

      await toggle(page).click();
      await setFullscreen(page, false);
      await expect
        .poll(async () => (await toggle(page).boundingBox())!.x)
        .toBe(windowedInset);
      expect(await left(page, "[data-title-tab-strip]")).toBeLessThan(210);
      if (platform !== "MacIntel") {
        await expect(
          page.getByRole("button", { name: "Close window" }),
        ).toBeVisible();
      }
    });
  });
}
