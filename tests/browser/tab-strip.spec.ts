import { expect, test } from "@playwright/test";

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
  for (let i = 0; i < 8 && (await right.count()); i++) {
    await right.click();
    await page.waitForTimeout(250);
  }
  await expect(right).toHaveCount(0);
  await expect(left).toBeVisible();
  expect((await add.boundingBox())?.x).toBe(pinned?.x);
  const viewport = await strip.boundingBox();
  expect(viewport!.x + viewport!.width).toBe(pinned!.x);
  for (let i = 0; i < 8 && (await left.count()); i++) {
    await left.click();
    await page.waitForTimeout(250);
  }
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
