import { expect, test } from "@playwright/test";

declare global {
  interface Window {
    reviewSteer: () => void;
    reviewAgain: () => void;
    reviewDelta: () => void;
    reviewFinish: () => void;
    reviewRestore: () => void;
  }
}

for (const mono of [false, true])
  for (const paced of [false, true]) {
    test(`steering preserves ordered Markdown boundaries (mono=${mono}, paced=${paced})`, async ({
      page,
    }) => {
      await page.goto(
        `/tests/browser/transcript-review.html?${mono ? "mono&" : ""}${paced ? "paced" : ""}`,
      );
      await expect(page.locator(".agent-transcript")).toBeVisible();
      await page.evaluate(() => window.reviewSteer());
      const messages = page.locator("[data-chat-message]");
      await expect(messages).toHaveText([
        "Plan the integration",
        "integration checklist",
        "Continue with tests",
        "Got it.",
      ]);
      await expect(messages.nth(1).locator("ol > li")).toHaveText(
        "integration checklist",
      );
      await expect(messages.nth(3).locator("ol")).toHaveCount(0);
      await page.evaluate(() => window.reviewAgain());
      await expect(messages.nth(4)).toHaveText("Keep it focused");
      await expect(messages.nth(5)).toHaveText("Testing now.");
      const ids = await messages.evaluateAll((els) =>
        els.map((el) => el.getAttribute("data-chat-message")),
      );
      await page.evaluate(() => window.reviewRestore());
      await expect(messages).toHaveCount(6);
      expect(
        await messages.evaluateAll((els) =>
          els.map((el) => el.getAttribute("data-chat-message")),
        ),
      ).toEqual(ids);
      const rects = await messages.evaluateAll((els) =>
        els.map((el) => {
          const r = el.getBoundingClientRect();
          return { top: r.top, bottom: r.bottom };
        }),
      );
      for (let i = 1; i < rects.length; i++)
        expect(rects[i].top).toBeGreaterThanOrEqual(rects[i - 1].bottom);
    });
  }

test("post-steer output preserves a reader's scroll position in earlier history", async ({
  page,
}) => {
  await page.goto("/tests/browser/transcript-review.html?mono&history");
  const scroller = page.locator(".agent-transcript");
  await expect(scroller).toBeVisible();
  await page.evaluate(() => window.reviewSteer());
  await expect
    .poll(() => scroller.evaluate((el) => el.scrollTop))
    .toBeGreaterThan(300);
  await scroller.hover();
  // Wheel input releases the transcript's live bottom pin.
  await page.mouse.wheel(0, -700);
  await expect
    .poll(() =>
      scroller.evaluate(
        (el) => el.scrollHeight - el.clientHeight - el.scrollTop,
      ),
    )
    .toBeGreaterThan(300);
  await page.waitForTimeout(150);
  const top = await scroller.evaluate((el) => el.scrollTop);
  await page.evaluate(() => window.reviewDelta());
  await expect.poll(() => scroller.evaluate((el) => el.scrollTop)).toBe(top);
});

for (const standalone of [false, true]) {
  test(`successful results expand independently in a narrow pane (standalone=${standalone})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 320, height: 800 });
    await page.goto(
      `/tests/browser/transcript-review.html?results${standalone ? "&standalone" : ""}`,
    );
    const triggers = page.getByRole("button", { name: /^Show result for/ });
    await expect(triggers).toHaveCount(6);
    await expect(page.locator("[data-tool-result]")).toHaveCount(0);
    await page
      .getByRole("button", {
        name: "Show result for Run structured result",
        exact: true,
      })
      .click();
    const json = page.getByRole("region", { name: "Tool result" }).first();
    await expect(json).toContainText('"ok": true');
    await expect
      .poll(() => json.locator(".markdown-code-token").count())
      .toBeGreaterThan(0);
    expect(await json.locator("pre").innerText()).toContain('\n  "ok": true,');
    await expect(json.locator("script")).toHaveCount(0);
    await page
      .getByRole("button", {
        name: "Show result for Run large output",
        exact: true,
      })
      .click();
    await expect(page.locator("[data-tool-result]")).toHaveCount(2);
    const large = page
      .locator("[data-tool-result]")
      .filter({ hasText: "Line 0:" });
    const geometry = await large.evaluate((el) => ({
      height: el.clientHeight,
      scroll: el.scrollHeight,
    }));
    expect(geometry.height).toBeLessThanOrEqual(288);
    expect(geometry.scroll).toBeGreaterThan(geometry.height);
    await large.evaluate((el) => {
      el.scrollTop = el.scrollHeight;
    });
    await expect
      .poll(() =>
        large.evaluate(
          (el) => el.scrollHeight - el.clientHeight - el.scrollTop,
        ),
      )
      .toBeLessThan(2);
    await expect(large).toContainText("Line 119:");
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth),
    ).toBeLessThanOrEqual(320);
    await page
      .getByRole("button", {
        name: "Hide result for Run structured result",
        exact: true,
      })
      .click();
    await expect(page.locator("[data-tool-result]")).toHaveCount(1);
    await page
      .getByRole("button", {
        name: "Show result for Run structured result",
        exact: true,
      })
      .click();
    await expect(page.locator("[data-tool-result]")).toHaveCount(2);
    if (!standalone) {
      await page.evaluate(() => window.reviewRestore());
      await expect(page.locator("[data-tool-result]")).toHaveCount(2);
    }
    await page
      .getByRole("button", {
        name: "Show result for MonoCode: List notes",
        exact: true,
      })
      .click();
    await expect(
      page.locator('[data-monocode-tool-call="notes.list"] [data-tool-result]'),
    ).toContainText('"Ideas"');
    await expect(
      page.getByRole("button", { name: /result for Run empty success/ }),
    ).toHaveCount(0);
    await expect(page.locator("body")).not.toContainText(
      "pending input arguments",
    );
    await page
      .getByRole("button", { name: "Show result for Find token", exact: true })
      .click();
    await expect(
      page.locator("[data-tool-result]").filter({ hasText: "unfinished" }).locator("[data-tool-result-text]"),
    ).toHaveText('{"unfinished":…');
  });
}

test("nested successful tools retain their own disclosures", async ({
  page,
}) => {
  await page.goto("/tests/browser/transcript-review.html?results&nested");
  await page
    .getByRole("button", { name: "Show Review's work", exact: true })
    .click();
  await page.getByRole("button", { name: /^Show the steps for/ }).click();
  await expect(
    page.getByRole("button", { name: /^Show result for/ }),
  ).toHaveCount(6);
  await page
    .getByRole("button", {
      name: "Show result for Read src/app.ts",
      exact: true,
    })
    .click();
  await expect(page.locator("[data-tool-result-text]")).toHaveText(
    "export const result = 42;",
  );
});

test("shell results show readable output and wheel gestures scroll the right container", async ({
  page,
}) => {
  await page.setViewportSize({ width: 420, height: 280 });
  await page.goto("/tests/browser/transcript-review.html?results&shell&scroll");
  await page
    .getByRole("button", { name: "Show result for Run structured result" })
    .click();
  const transcript = page.locator(".agent-transcript");
  const result = page.getByRole("region", { name: "Tool result" }).first();
  await expect(result).toContainText("git status");
  await expect(result).toContainText("Exit 1");
  await expect(result).toContainText("fatal: not a git repository");
  await expect(result).not.toContainText('"combined_output"');
  await result.getByRole("button", { name: "View raw JSON" }).click();
  await expect(result.locator("[data-tool-result-raw]")).toContainText(
    '"combined_output"',
  );
  await result.getByRole("button", { name: "Hide raw JSON" }).click();

  // A short result hands the wheel to its surrounding activity list before
  // the conversation. The transcript must stay put while reading that list.
  const activity = page.locator(".zen-phase-live");
  const geometry = await activity.evaluate((el) => ({
    height: el.clientHeight,
    scroll: el.scrollHeight,
  }));
  expect(geometry.scroll).toBeGreaterThan(geometry.height);
  await result.scrollIntoViewIfNeeded();
  const chatTop = await transcript.evaluate((el) => el.scrollTop);
  const before = await activity.evaluate((el) => ({
    top: el.scrollTop,
    max: el.scrollHeight - el.clientHeight,
  }));
  const delta = before.top > 5 ? -90 : 90;
  const box = await result.boundingBox();
  expect(box).not.toBeNull();
  await page.mouse.move(
    box!.x + box!.width / 2,
    box!.y + Math.min(box!.height / 2, 40),
  );
  await page.mouse.wheel(0, delta);
  if (delta > 0)
    await expect
      .poll(() => activity.evaluate((el) => el.scrollTop))
      .toBeGreaterThan(before.top);
  else
    await expect
      .poll(() => activity.evaluate((el) => el.scrollTop))
      .toBeLessThan(before.top);
  await expect.poll(() => result.evaluate((el) => el.scrollTop)).toBe(0);
  expect(await transcript.evaluate((el) => el.scrollTop)).toBe(chatTop);
  const afterUp = await activity.evaluate((el) => el.scrollTop);
  const shortBox = await result.boundingBox();
  expect(shortBox).not.toBeNull();
  await page.mouse.move(shortBox!.x + shortBox!.width / 2, shortBox!.y + 12);
  await page.mouse.wheel(0, 50);
  await expect
    .poll(() => activity.evaluate((el) => el.scrollTop))
    .toBeGreaterThan(afterUp);
  expect(await transcript.evaluate((el) => el.scrollTop)).toBe(chatTop);

  // A genuinely long result has its own scroll range and takes the wheel until
  // it reaches the edge, without prematurely scrolling the conversation.
  await page
    .getByRole("button", { name: "Show result for Run large output" })
    .click();
  const large = page
    .locator("[data-tool-result]")
    .filter({ hasText: "Line 0:" });
  await large.scrollIntoViewIfNeeded();
  await large.evaluate((el) => {
    el.scrollTop = 0;
  });
  const beforeParent = await transcript.evaluate((el) => el.scrollTop);
  const hoverLarge = async () => {
    const point = await large.evaluate((el) => {
      const result = el.getBoundingClientRect();
      const activity = el.closest(".zen-phase-live")!.getBoundingClientRect();
      const chat = el.closest(".agent-transcript")!.getBoundingClientRect();
      const top = Math.max(result.top, activity.top, chat.top, 0);
      const bottom = Math.min(result.bottom, activity.bottom, chat.bottom, innerHeight);
      return { x: result.x + result.width / 2, y: (top + bottom) / 2, height: bottom - top };
    });
    expect(point.height).toBeGreaterThan(0);
    await page.mouse.move(point.x, point.y);
  };
  await hoverLarge();
  await page.mouse.wheel(0, 100);
  await expect
    .poll(() => large.evaluate((el) => el.scrollTop))
    .toBeGreaterThan(0);
  expect(await transcript.evaluate((el) => el.scrollTop)).toBe(beforeParent);

  // Both output edges hand scrolling to the enclosing activity window.
  for (const direction of [-1, 1]) {
    await large.evaluate((el, direction) => {
      el.scrollTop = direction < 0 ? 0 : el.scrollHeight;
      const activity = el.closest(".zen-phase-live")!;
      activity.scrollTop = (activity.scrollHeight - activity.clientHeight) / 2;
    }, direction);
    await hoverLarge();
    const workTop = await activity.evaluate((el) => el.scrollTop);
    const outputTop = await large.evaluate((el) => el.scrollTop);
    const chatTop = await transcript.evaluate((el) => el.scrollTop);
    await page.mouse.wheel(0, direction * 40);
    await expect.poll(async () => (await activity.evaluate((el) => el.scrollTop)) - workTop)
      [direction < 0 ? "toBeLessThan" : "toBeGreaterThan"](0);
    expect(await large.evaluate((el) => el.scrollTop)).toBe(outputTop);
    expect(await transcript.evaluate((el) => el.scrollTop)).toBe(chatTop);
  }
});

test("previously capped shell JSON stays readable without breaking the transcript layout", async ({
  page,
}) => {
  await page.setViewportSize({ width: 340, height: 480 });
  await page.goto("/tests/browser/transcript-review.html?results&capped-shell");
  await page
    .getByRole("button", { name: "Show result for Run structured result" })
    .click();

  const result = page.getByRole("region", { name: "Tool result" }).first();
  await expect(result).toContainText('rg --stats "unsafe|unwrap|expect|panic|TODO|FIXME" src/');
  await expect(result).toContainText("Exit 0");
  await expect(result).toContainText("src/main.rs:1");
  await expect(result).not.toContainText('"combinedOutput"');
  await expect(result).not.toContainText('\\n');
  await expect(result.getByRole("button", { name: "View raw JSON" })).toBeVisible();

  const metrics = await result.evaluate((el) => ({
    viewport: document.documentElement.clientWidth,
    documentWidth: document.documentElement.scrollWidth,
    panelWidth: el.getBoundingClientRect().width,
    panelHeight: el.clientHeight,
    contentHeight: el.scrollHeight,
  }));
  expect(metrics.documentWidth).toBeLessThanOrEqual(metrics.viewport);
  expect(metrics.panelWidth).toBeLessThanOrEqual(metrics.viewport);
  expect(metrics.contentHeight).toBeGreaterThan(metrics.panelHeight);
});

test("PowerShell table columns remain aligned inside a narrow output panel", async ({
  page,
}) => {
  await page.setViewportSize({ width: 320, height: 450 });
  await page.goto("/tests/browser/transcript-review.html?results&table-shell");
  await page
    .getByRole("button", { name: "Show result for Run structured result" })
    .click();

  const result = page.getByRole("region", { name: "Tool result" }).first();
  await expect(result).toContainText("Get-ChildItem");
  await expect(result).toContainText("Exit 0");
  const table = result.locator("pre").first();
  await expect(table).toContainText("LastWriteTime");
  await expect(table).toContainText("inspection");
  const widths = await result.evaluate((el) => {
    const pre = el.querySelector("pre")!;
    return {
      whiteSpace: getComputedStyle(pre).whiteSpace,
      tableWidth: pre.clientWidth,
      tableContentWidth: pre.scrollWidth,
      pageWidth: document.documentElement.scrollWidth,
      viewportWidth: document.documentElement.clientWidth,
    };
  });
  expect(widths.whiteSpace).toBe("pre");
  expect(widths.tableContentWidth).toBeGreaterThan(widths.tableWidth);
  expect(widths.pageWidth).toBeLessThanOrEqual(widths.viewportWidth);
});

test("OpenCode search and shell results use consistent compact result cards without invented exit codes", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 550 });
  await page.goto("/tests/browser/transcript-review.html?results&opencode");
  await page.getByRole("button", { name: "Show result for Find TODO|FIXME|HACK|XXX" }).click();

  const search = page.getByRole("region", { name: "Tool result" }).first();
  await expect(search).toContainText("Find TODO|FIXME|HACK|XXX");
  await expect(search).toContainText("Found 47 matches");
  await expect(search).toContainText("Line 7937:");
  await expect(search).not.toContainText("Exit 0");
  expect(await search.evaluate((el) => getComputedStyle(el.querySelector("pre")!).whiteSpace)).toBe("pre");

  await page.getByRole("button", { name: "Show result for Read src/main.rs" }).click();
  const read = page.getByRole("region", { name: "Tool result" }).nth(1);
  await expect(read).toContainText("Read src/main.rs");
  await expect(read).toContainText('println!("hello")');
  await expect(read).not.toContainText("Exit 0");

  await page.getByRole("button", { name: "Show result for git status" }).click();
  const shell = page.getByRole("region", { name: "Tool result" }).last();
  await expect(shell).toContainText("git status");
  await expect(shell).toContainText("fatal: not a git repository");
  await expect(shell).not.toContainText("Exit 1");
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
});
