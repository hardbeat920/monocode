import { expect, test, type Locator } from "@playwright/test";

async function setOverflow(strip: Locator, overflow: number) {
  const width = await strip.evaluate((el, extra) => {
    const row = el.parentElement!;
    const fixture = row.parentElement!;
    const inset = fixture.getBoundingClientRect().width - row.clientWidth;
    const add = row.querySelector<HTMLButtonElement>(
      '[aria-label="Connect an inbox source"]',
    );
    const width =
      el.firstElementChild!.scrollWidth + (add?.offsetWidth ?? 0) - extra;
    fixture.style.width = `${width + inset}px`;
    el.scrollLeft = 0;
    return width;
  }, overflow);
  await expect
    .poll(() => strip.evaluate((el) => el.parentElement!.clientWidth))
    .toBe(width);
}

test.beforeEach(async ({ page }) => {
  await page.goto("/tests/browser/tab-strip.html");
});

test("tabs stay separate, arrows track edges, and the compact add action stays pinned", async ({
  page,
}) => {
  const left = page.getByRole("button", { name: "Scroll tabs left" });
  const right = page.getByRole("button", { name: "Scroll tabs right" });
  const add = page.getByRole("button", { name: "Connect an inbox source" });
  await expect(left).toHaveCount(0);
  await expect(right).toBeVisible();
  await expect(add.getByText("Add connection")).toBeHidden();
  const strip = page.getByRole("tablist");
  const assertNoLeftSpacer = async () => {
    const gap = await strip.evaluate(
      (el) =>
        el.getBoundingClientRect().left -
        el.parentElement!.getBoundingClientRect().left,
    );
    expect(gap).toBe(0);
  };
  await assertNoLeftSpacer();
  const pinned = await add.boundingBox();
  const boxes = await page.getByRole("tab").evaluateAll((tabs) =>
    tabs.map((tab) => {
      const box = tab.getBoundingClientRect();
      return { left: box.left, right: box.right };
    }),
  );
  for (let i = 1; i < boxes.length; i++)
    expect(boxes[i].left).toBeGreaterThanOrEqual(boxes[i - 1].right);
  await right.click();
  await expect(right).toHaveCount(0);
  await expect(left).toBeVisible();
  expect((await add.boundingBox())?.x).toBe(pinned?.x);
  const viewport = await strip.boundingBox();
  expect(viewport!.x + viewport!.width).toBe(pinned!.x);
  await left.click();
  await expect(left).toHaveCount(0);
  await expect(right).toBeVisible();
  await assertNoLeftSpacer();
});

test("resizing and removing connections restore the full add label and hide arrows", async ({
  page,
}) => {
  const addLabel = page
    .getByRole("button", { name: "Connect an inbox source" })
    .getByText("Add connection");
  await page.getByRole("button", { name: "Expand", exact: true }).click();
  await expect(addLabel).toBeVisible();
  await expect(page.getByRole("button", { name: /Scroll tabs/ })).toHaveCount(
    0,
  );
  await page.getByRole("button", { name: "Narrow", exact: true }).click();
  await expect(addLabel).toBeHidden();
  await page.getByRole("button", { name: "Remove connections" }).click();
  await expect(addLabel).toBeVisible();
  await expect(page.getByRole("button", { name: /Scroll tabs/ })).toHaveCount(
    0,
  );
});

test("selecting a hidden tab brings it into the viewport", async ({ page }) => {
  await page.getByRole("button", { name: "Select last" }).click();
  await expect(
    page.getByRole("button", { name: "Scroll tabs right" }),
  ).toHaveCount(0);
  const tab = await page
    .getByRole("tab", { name: "Jira", exact: true })
    .boundingBox();
  const strip = await page.getByRole("tablist").boundingBox();
  expect(tab!.x).toBeGreaterThanOrEqual(strip!.x - 1);
  expect(tab!.x + tab!.width).toBeLessThanOrEqual(strip!.x + strip!.width + 1);
});

for (const overflow of [2, 8, 25, 60]) {
  test(`one click reaches either edge with ${overflow}px of overflow`, async ({
    page,
  }) => {
    const strip = page.getByRole("tablist");
    const left = page.getByRole("button", { name: "Scroll tabs left" });
    const right = page.getByRole("button", { name: "Scroll tabs right" });
    await setOverflow(strip, overflow);
    await expect(left).toHaveCount(0);
    await expect(right).toBeVisible();
    const first = page.getByRole("tab", { name: "GitHub", exact: true });
    const initialX = (await first.boundingBox())!.x;
    await right.click();
    await expect(right).toHaveCount(0);
    await expect(left).toBeVisible();
    await expect
      .poll(async () => (await first.boundingBox())!.x)
      .toBeCloseTo(initialX - overflow, 0);
    const last = await page
      .getByRole("tab", { name: "Jira", exact: true })
      .boundingBox();
    const viewport = await strip.boundingBox();
    expect(last!.x + last!.width).toBeLessThanOrEqual(
      viewport!.x + viewport!.width + 1,
    );
    await left.click();
    await expect(left).toHaveCount(0);
    await expect(right).toBeVisible();
    await expect
      .poll(async () => (await first.boundingBox())!.x)
      .toBeCloseTo(initialX, 0);
  });
}

for (const hasAdd of [true, false]) {
  for (const overflow of [0, 1, 2]) {
    test(`${hasAdd ? "pinned add" : "no add"} at ${overflow}px overflow`, async ({
      page,
    }) => {
      if (!hasAdd) await page.goto("/tests/browser/tab-strip.html?no-add");
      const strip = page.getByRole("tablist");
      await setOverflow(strip, overflow);
      const right = page.getByRole("button", { name: "Scroll tabs right" });
      const left = page.getByRole("button", { name: "Scroll tabs left" });
      await expect(left).toHaveCount(0);
      if (overflow <= 1) {
        await expect(right).toHaveCount(0);
        expect(
          await strip.evaluate((el) => el.scrollWidth - el.clientWidth),
        ).toBe(overflow);
      } else {
        await expect(right).toBeVisible();
        await right.click();
        await expect(right).toHaveCount(0);
        await expect(left).toBeVisible();
        await left.click();
        await expect(left).toHaveCount(0);
        await expect(right).toBeVisible();
      }
    });
  }
}

test("reduced motion still reaches either edge in one click", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await setOverflow(page.getByRole("tablist"), 25);
  const right = page.getByRole("button", { name: "Scroll tabs right" });
  const left = page.getByRole("button", { name: "Scroll tabs left" });
  const clickWithoutMotion = async (button: Locator, distance: number) => {
    const movement = await button.evaluate(async (el) => {
      const first =
        el.parentElement!.querySelector<HTMLElement>('[role="tab"]')!;
      const before = first.getBoundingClientRect().left;
      (el as HTMLButtonElement).click();
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
      return first.getBoundingClientRect().left - before;
    });
    expect(movement).toBeCloseTo(distance, 0);
  };
  await clickWithoutMotion(right, -25);
  await expect(right).toHaveCount(0);
  await expect(left).toBeVisible();
  await clickWithoutMotion(left, 25);
  await expect(left).toHaveCount(0);
  await expect(right).toBeVisible();
});
