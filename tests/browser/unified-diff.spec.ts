import { expect, test, type Page } from "@playwright/test";

test.use({ viewport: { width: 1100, height: 760 } });
type FixtureAPI = {
  diffMetrics: () => {
    commits: number;
    renderMs: number;
    lastCommitAt: number;
    nodes: number;
    stages: { id: string; pos?: number }[];
  };
  resetDiffMetrics: () => void;
  focusDiffFile: (index: number) => void;
  refreshDiff: () => void;
  growFirstDiff: () => void;
  removeDiffFile: (id: string) => void;
  expectedFirstStage: () => number;
  useManyDiffFiles: () => void;
  useHugeDiffFile: () => void;
  setDiffVisible: (visible: boolean) => void;
  useLightDiffTheme: () => void;
};
declare global {
  interface Window extends FixtureAPI {}
}
type NoArgumentAction = {
  [Key in keyof FixtureAPI]: Parameters<FixtureAPI[Key]> extends []
    ? Key
    : never;
}[keyof FixtureAPI];
const fixture = <Key extends NoArgumentAction>(page: Page, action: Key) =>
  page.evaluate((action) => window[action](), action) as Promise<
    ReturnType<FixtureAPI[Key]>
  >;

test("reads and stages changes with stable gutters and native scrolling", async ({
  page,
}, info) => {
  await page.goto("/tests/browser/unified-diff.html");
  const surface = page.getByRole("region", {
    name: "Unified diff",
    exact: true,
  });
  const horizontal = page.getByRole("region", {
    name: "Scroll diff horizontally",
  });
  await expect(page.locator("[data-diff-code] [style]").first()).toBeVisible();
  for (const scroller of [
    page.locator(".diff-review-code-scroll").first(),
    horizontal,
  ]) {
    const gestures = await scroller.evaluate((node) => {
      const send = (deltaX: number, deltaY: number) => {
        const wheel = new WheelEvent("wheel", {
          deltaX,
          deltaY,
          bubbles: true,
          cancelable: true,
        });
        node.dispatchEvent(wheel);
        return wheel.defaultPrevented;
      };
      node.scrollLeft = 0;
      const left = send(-120, 1);
      const inward = send(120, 1);
      const vertical = send(-1, 120);
      node.scrollLeft = node.scrollWidth - node.clientWidth;
      const right = send(120, 1);
      node.scrollLeft = 0;
      return { left, right, inward, vertical };
    });
    expect(gestures).toEqual({
      left: true,
      right: true,
      inward: false,
      vertical: false,
    });
  }
  const selectedLines = page.locator("[data-diff-code]");
  const expectedSelection = await selectedLines.evaluateAll((nodes) =>
    nodes
      .slice(1, 5)
      .map((node) => node.textContent)
      .join("\n"),
  );
  const endpoints = await selectedLines.evaluateAll((nodes) => {
    function caret(node: Element, end: boolean) {
      const texts = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
      let text = texts.nextNode()!;
      if (end) while (texts.nextNode()) text = texts.currentNode;
      const range = document.createRange();
      range.setStart(text, end ? text.textContent!.length : 0);
      range.collapse(true);
      const rect = range.getBoundingClientRect();
      return { x: rect.x, y: rect.y + rect.height / 2 };
    }
    return { start: caret(nodes[1], false), end: caret(nodes[4], true) };
  });
  await page.mouse.move(endpoints.start.x + 0.1, endpoints.start.y);
  await page.mouse.down();
  await page.mouse.move(endpoints.end.x, endpoints.end.y, { steps: 8 });
  await page.mouse.up();
  expect(await page.evaluate(() => getSelection()?.toString())).toBe(
    expectedSelection,
  );
  await page.evaluate(() => getSelection()?.removeAllRanges());
  const added = page
    .locator(".diff-review-gutter-lane .diff-review-line.is-add")
    .first();
  await expect(added.locator(".diff-review-number").first()).toHaveText("");
  await expect(added.locator(".diff-review-number").nth(1)).toHaveText("13");
  const deleted = page
    .locator(".diff-review-gutter-lane .diff-review-line.is-del")
    .first();
  await expect(deleted.locator(".diff-review-number").first()).toHaveText("13");
  await expect(deleted.locator(".diff-review-number").nth(1)).toHaveText("");
  await added.hover();
  await added.getByRole("button", { name: "Stage hunk", exact: true }).click();
  expect((await fixture(page, "diffMetrics")).stages[0]).toEqual({
    id: "file-0",
    pos: await fixture(page, "expectedFirstStage"),
  });
  await added
    .getByRole("button", { name: "Comment on line 13", exact: true })
    .click();
  await expect(page.getByRole("dialog", { name: /Comment on/ })).toBeVisible();
  await page.getByRole("button", { name: "Cancel comment" }).click();

  const gutter = added.locator(".diff-review-gutter");
  const before = (await gutter.boundingBox())!.x;
  const hunkLabel = page.locator("[data-diff-hunk]").first().locator("span");
  const labelLeft = (await hunkLabel.boundingBox())!.x;
  await page.mouse.move(500, 250);
  // A real trackpad gesture usually has a small delta on the other axis.
  await page.mouse.wheel(180, -1);
  await expect
    .poll(() => horizontal.evaluate((node) => node.scrollLeft))
    .toBeGreaterThan(100);
  await horizontal.evaluate((node) => {
    node.scrollLeft = 450;
  });
  await expect.poll(async () => (await gutter.boundingBox())!.x).toBe(before);
  expect(await horizontal.evaluate((node) => node.scrollLeft)).toBeGreaterThan(
    400,
  );
  await expect
    .poll(() =>
      page
        .locator(".diff-review-code-scroll")
        .first()
        .evaluate((node) => node.scrollLeft),
    )
    .toBeGreaterThan(400);
  await expect
    .poll(async () => (await hunkLabel.boundingBox())!.x)
    .toBeCloseTo(labelLeft, 0);
  await page.mouse.move(500, 250);
  await page.mouse.wheel(0, 300);
  await expect
    .poll(() => surface.evaluate((node) => node.scrollTop))
    .toBeGreaterThan(100);
  const sticky = page.locator("[data-sticky-file='file-0']");
  await expect(sticky).toBeVisible();
  const top = await surface.evaluate((node) => node.scrollTop);
  await sticky.hover();
  await page.mouse.wheel(0, 180);
  await expect
    .poll(() => surface.evaluate((node) => node.scrollTop))
    .toBeGreaterThan(top + 50);
  const transition = await surface.evaluate((node) => {
    const header = node.querySelector("[data-diff-file-id='file-0']")!;
    const next = node.querySelector("[data-diff-file-id='file-1']")!;
    const content = node.querySelector(".diff-review-content")!;
    const nextTop =
      next.getBoundingClientRect().top - content.getBoundingClientRect().top;
    // Read in the same task, before React or an animation frame can catch up.
    node.scrollTop = nextTop - 16;
    node.querySelector<HTMLDivElement>(".diff-review-code-scroll")!.scrollLeft =
      700;
    return {
      currentBottom:
        header.getBoundingClientRect().bottom -
        node.getBoundingClientRect().top,
      nextTop:
        next.getBoundingClientRect().top - node.getBoundingClientRect().top,
      headerLeft:
        header.getBoundingClientRect().left - node.getBoundingClientRect().left,
      gutterLeft:
        node.querySelector(".diff-review-gutter")!.getBoundingClientRect()
          .left - node.getBoundingClientRect().left,
    };
  });
  expect(transition.currentBottom).toBeCloseTo(transition.nextTop, 0);
  expect(transition.nextTop).toBeCloseTo(16, 0);
  expect(transition.headerLeft).toBeCloseTo(0, 0);
  expect(transition.gutterLeft).toBeCloseTo(0, 0);
  await page.screenshot({
    path: info.outputPath("unified-horizontal-sticky.png"),
  });
  await surface.evaluate((node) => {
    node.scrollTop = 0;
  });
  await horizontal.evaluate((node) => {
    node.scrollLeft = 0;
  });
  await expect(page.locator("[data-diff-file-id='file-0']")).toBeVisible();
  await page.screenshot({ path: info.outputPath("unified-dark.png") });
  const darkInk = await page
    .locator("[data-diff-code] [style]")
    .first()
    .evaluate((node) => getComputedStyle(node).color);
  await fixture(page, "useLightDiffTheme");
  await expect
    .poll(() =>
      page
        .locator("[data-diff-code] [style]")
        .first()
        .evaluate((node) => getComputedStyle(node).color),
    )
    .not.toBe(darkInk);
  await page.screenshot({ path: info.outputPath("unified-light.png") });
});

test("navigates hunks across folded files and preserves folds on refresh", async ({
  page,
}) => {
  await page.goto("/tests/browser/unified-diff.html");
  const position = page.getByLabel("Hunk position");
  await expect(position).toHaveText("1/144");
  await page.getByRole("button", { name: "Next hunk", exact: true }).click();
  await expect(position).toHaveText("2/144");
  await page.keyboard.press("Alt+ArrowDown");
  await expect(position).toHaveText("3/144");
  await page
    .getByRole("button", { name: "Previous hunk", exact: true })
    .click();
  await expect(position).toHaveText("2/144");
  await page
    .getByRole("button", { name: "Collapse all files", exact: true })
    .click();
  await expect(page.locator("[data-diff-line]")).toHaveCount(0);
  await expect(position).toHaveText("0/144");
  await page.getByRole("button", { name: "Next hunk", exact: true }).click();
  await expect(position).toHaveText("1/144");
  await expect(page.locator("[data-diff-line]").first()).toBeVisible();
  await fixture(page, "refreshDiff");
  await page.evaluate(() => window.removeDiffFile("file-47"));
  await expect(
    page.getByRole("button", {
      name: "Expand src/review/file-1.ts",
      exact: true,
    }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Expand all files", exact: true })
    .click();
  await page.evaluate(() => window.focusDiffFile(20));
  const focused = page.locator("[data-diff-file-id='file-20']");
  await expect(focused).toBeVisible();
  const before = (await focused.boundingBox())!.y;
  await page.evaluate(() => window.setDiffVisible(false));
  await expect(page.locator("[data-diff-line]")).toHaveCount(0);
  await page.evaluate(() => window.setDiffVisible(true));
  await expect(focused).toBeVisible();
  await expect
    .poll(async () => (await focused.boundingBox())!.y)
    .toBeCloseTo(before, 0);
  await fixture(page, "growFirstDiff");
  await expect
    .poll(async () => (await focused.boundingBox())!.y)
    .toBeCloseTo(before, 0);
});

test("keeps rendering bounded for hundreds of files and a huge patch without hover commits", async ({
  page,
}, info) => {
  await page.goto("/tests/browser/unified-diff.html");
  await expect(page.locator("[data-diff-code] [style]").first()).toBeVisible();
  await page.waitForFunction(
    () => performance.now() - window.diffMetrics().lastCommitAt > 200,
  );
  await fixture(page, "resetDiffMetrics");
  for (let index = 0; index < 20; index++) {
    await page.mouse.move(240, 145 + (index % 10) * 22);
    await page.evaluate(() => new Promise(requestAnimationFrame));
  }
  expect((await fixture(page, "diffMetrics")).commits).toBe(0);
  const horizontal = page.getByRole("region", {
    name: "Scroll diff horizontally",
  });
  for (const left of [200, 600, 1000, 450, 0]) {
    await horizontal.evaluate((node, left) => {
      node.scrollLeft = left;
    }, left);
    await page.evaluate(() => new Promise(requestAnimationFrame));
  }
  expect((await fixture(page, "diffMetrics")).commits).toBe(0);
  for (const width of [1000, 840, 960, 780, 1100]) {
    await page.setViewportSize({ width, height: 760 });
    await page.evaluate(() => new Promise(requestAnimationFrame));
  }
  expect((await fixture(page, "diffMetrics")).commits).toBe(0);
  const dragCost = await page.evaluate(async () => {
    const original = Element.prototype.getBoundingClientRect;
    let geometryReads = 0;
    Element.prototype.getBoundingClientRect = function () {
      if (this.matches(".diff-review-scroll, .diff-review-content"))
        geometryReads++;
      return original.call(this);
    };
    const review = document.querySelector<HTMLElement>(".diff-review")!;
    const previous = { flex: review.style.flex, width: review.style.width };
    try {
      review.style.flex = "none";
      for (let index = 0; index < 30; index++) {
        review.style.width = `${780 + (index % 15) * 20}px`;
        await new Promise(requestAnimationFrame);
      }
      await new Promise(requestAnimationFrame);
      return { geometryReads, commits: window.diffMetrics().commits };
    } finally {
      Element.prototype.getBoundingClientRect = original;
      review.style.flex = previous.flex;
      review.style.width = previous.width;
    }
  });
  expect(dragCost).toEqual({ geometryReads: 0, commits: 0 });
  // A window drag also nudges the height; overscan covers it without React.
  for (const height of [740, 700, 650, 720, 760]) {
    await page.setViewportSize({ width: 1100, height });
    await page.evaluate(() => new Promise(requestAnimationFrame));
  }
  expect((await fixture(page, "diffMetrics")).commits).toBe(0);
  const rowsBeforeHeightChange = await page.locator("[data-diff-line]").count();
  await page.setViewportSize({ width: 1100, height: 1200 });
  await expect
    .poll(() => page.locator("[data-diff-line]").count())
    .toBeGreaterThan(rowsBeforeHeightChange);
  await page.setViewportSize({ width: 1100, height: 760 });
  await fixture(page, "useManyDiffFiles");
  await expect(page.getByRole("toolbar", { name: "Diff tools" })).toContainText(
    "500 files",
  );
  await expect
    .poll(() => page.locator("[data-diff-line]").count())
    .toBeGreaterThan(10);
  expect(await page.locator("[data-diff-line]").count()).toBeLessThan(120);
  expect((await fixture(page, "diffMetrics")).nodes).toBeLessThan(3500);
  const surface = page.getByRole("region", { name: "Unified diff" });
  for (const fraction of [0.2, 0.5, 0.85]) {
    await surface.evaluate((node, fraction) => {
      node.scrollTop = node.scrollHeight * fraction;
    }, fraction);
    await page.waitForFunction(() =>
      [...document.querySelectorAll("[data-diff-line]")].some((node) => {
        const rect = node.getBoundingClientRect();
        return rect.top > 32 && rect.bottom < innerHeight;
      }),
    );
    expect(await page.locator("[data-diff-line]").count()).toBeLessThan(120);
  }
  await fixture(page, "useHugeDiffFile");
  await expect(page.getByRole("toolbar", { name: "Diff tools" })).toContainText(
    "1 file",
  );
  await expect
    .poll(() => page.locator("[data-diff-line]").count())
    .toBeGreaterThan(10);
  await surface.evaluate((node) => {
    node.scrollTop = node.scrollHeight * 0.8;
  });
  await expect
    .poll(() => page.locator("[data-diff-code]").first().textContent())
    .toMatch(/large1[45]\d{3}/);
  expect(await page.locator("[data-diff-line]").count()).toBeLessThan(120);
  await info.attach("rendering-metrics", {
    body: JSON.stringify(await fixture(page, "diffMetrics")),
    contentType: "application/json",
  });
});

test("virtualizes embedded cards against the parent scroller", async ({
  page,
}) => {
  await page.goto("/tests/browser/unified-diff.html?embedded&light");
  const parent = page.locator("[data-diff-fixture-scroll]");
  await expect(page.locator("[data-diff-line]").first()).toBeVisible();
  await page.mouse.move(500, 500);
  await page.mouse.wheel(0, 500);
  await expect
    .poll(() => parent.evaluate((node) => node.scrollTop))
    .toBeGreaterThan(200);
  expect(await page.locator("[data-diff-line]").count()).toBeLessThan(120);
  await page.evaluate(() => window.focusDiffFile(30));
  await expect(page.locator("[data-diff-file-id='file-30']")).toBeVisible();
});
