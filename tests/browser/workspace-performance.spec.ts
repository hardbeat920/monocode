import { expect, test } from "@playwright/test";
import type { ReaderAnchor } from "./readerAnchor";

type WorkspaceFixture = Window & {
  workspaceHasStore: () => boolean;
  workspaceBeginStreams: () => void;
  workspaceFinishStreams: () => void;
  workspaceResetMetrics: () => void;
  workspaceMetrics: () => unknown;
  captureReaderAnchor: () => ReaderAnchor;
  readerAnchorTop: (anchor: ReaderAnchor) => number;
};

// Keep the layout invitation from covering workspace controls during measurements.
// Invitation behavior is covered separately by layout-opt-in.spec.ts.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("monocode.newLayoutPromptDismissed", "1");
  });
});

test("switches six large sessions through the workspace shortcut", async ({
  page,
}, testInfo) => {
  test.skip(
    !process.env.MONOCODE_PERFORMANCE,
    "Run with the release performance configuration",
  );
  test.setTimeout(120_000);
  const errors: string[] = [];
  page.on("pageerror", (error) => {
    errors.push(error.message);
    console.log(error.message);
  });
  page.on("console", (message) => {
    if (message.type() === "error") console.log(message.text());
  });
  await page.goto(
    `/tests/browser/workspace-performance.html?${process.env.TRACE_LAYOUT ? "traceLayout&" : ""}${process.env.REACT_PROFILE ? "profile" : ""}`,
  );
  await expect(
    page.locator('[data-session-drop="workspace-session-0"]'),
  ).toBeVisible({ timeout: 60_000 });
  await page.evaluate(() => document.fonts.ready);
  if (process.env.STREAMING) {
    await page.keyboard.press("Meta+t");
    await page.waitForFunction(() =>
      (window as WorkspaceFixture).workspaceHasStore?.(),
    );
    await page.keyboard.press("Meta+w");
    await expect(
      page.locator('[data-session-drop="workspace-session-0"]'),
    ).toBeVisible();
  }
  const samples: number[] = [];
  for (let visit = 0; visit < 18; visit++) {
    const index = (visit + 1) % 6;
    await page.evaluate((index) => {
      const benchmark = window as unknown as { switchResult: number | null };
      benchmark.switchResult = null;
      const onKey = (event: KeyboardEvent) => {
        if (!event.metaKey || event.key !== String(index + 1)) return;
        window.removeEventListener("keydown", onKey, true);
        const start = (window as unknown as { workspaceKeyAt: number })
          .workspaceKeyAt;
        const pane = document.querySelector<HTMLElement>(
          `[data-session-drop="workspace-session-${index}"]`,
        )!;
        const wait = () => {
          if (performance.now() - start > 3000) {
            benchmark.switchResult = 3000;
            return;
          }
          if (!pane.checkVisibility({ opacityProperty: true })) {
            requestAnimationFrame(wait);
            return;
          }
          requestAnimationFrame(() => {
            benchmark.switchResult = performance.now() - start;
          });
        };
        requestAnimationFrame(wait);
      };
      window.addEventListener("keydown", onKey, true);
    }, index);
    await page.keyboard.press(`Meta+${index + 1}`);
    await page.waitForFunction(
      () =>
        (window as unknown as { switchResult: number | null }).switchResult !==
        null,
    );
    samples.push(
      await page.evaluate(
        () => (window as unknown as { switchResult: number }).switchResult,
      ),
    );
    if (visit < 6) {
      await expect(
        page.locator(
          `[data-session-drop="workspace-session-${index}"] [data-transcript-turn]`,
        ),
      ).toHaveCount(20, { timeout: 30_000 });
      await expect(
        page
          .locator(
            `[data-session-drop="workspace-session-${index}"] .markdown-code-shell`,
          )
          .last()
          .locator(".markdown-code-token")
          .first(),
      ).toHaveCount(1, { timeout: 30_000 });
    }
    if (visit === 5 && process.env.STREAMING)
      await page.evaluate(() => {
        (window as WorkspaceFixture).workspaceBeginStreams();
        (window as WorkspaceFixture).workspaceResetMetrics();
      });
  }
  await testInfo.attach("workspace-switch", {
    body: JSON.stringify({ samples, errors }),
    contentType: "application/json",
  });
  console.log(
    JSON.stringify({
      samples,
      errors,
      commits: await page.evaluate(
        () =>
          (window as unknown as { workspaceCommits: unknown }).workspaceCommits,
      ),
    }),
  );
  const navigation = await page.evaluate(() =>
    performance.getEntriesByName("monocode:tab-switch").map((entry) => ({
      duration: entry.duration,
      detail: (entry as PerformanceMeasure).detail,
    })),
  );
  console.log("navigation", JSON.stringify(navigation));
  if (process.env.TRACE_LAYOUT)
    console.log(
      "layout",
      JSON.stringify(
        await page.evaluate(
          () =>
            (window as unknown as { workspaceLayoutReads: unknown })
              .workspaceLayoutReads,
        ),
      ),
    );
  if (process.env.REACT_PROFILE)
    console.log(
      "profiles",
      JSON.stringify(
        await page.evaluate(
          () =>
            (window as unknown as { workspaceProfiles: unknown })
              .workspaceProfiles,
        ),
      ),
    );
  await testInfo.attach("navigation", {
    body: JSON.stringify(navigation),
    contentType: "application/json",
  });
  console.log(
    "DOM",
    await page.evaluate(() =>
      [...document.querySelectorAll(".agent-transcript")].map((el) => ({
        nodes: el.querySelectorAll("*").length,
        turns: el.querySelectorAll("[data-transcript-turn]").length,
        tokens: el.querySelectorAll(".markdown-code-token").length,
      })),
    ),
  );
  const warm = samples.slice(6).sort((a, b) => a - b);
  const p95 = warm[Math.ceil(warm.length * 0.95) - 1];
  const budget = Number(process.env.TAB_SWITCH_BUDGET_MS ?? 50);
  await testInfo.attach("acceptance", {
    body: JSON.stringify({
      warmP95Ms: p95,
      budgetMs: budget,
      passed: p95 <= budget,
    }),
    contentType: "application/json",
  });
  expect(p95).toBeLessThanOrEqual(budget);
  expect(errors).toEqual([]);
});

test("preserves following and a reader's position through real file-pane resizing", async ({
  page,
}, testInfo) => {
  test.setTimeout(90_000);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  await page.goto("/tests/browser/workspace-performance.html");
  const pane = page.locator('[data-session-drop="workspace-session-0"]');
  await expect(pane).toBeVisible({ timeout: 60_000 });
  await page.keyboard.press("Meta+t");
  await page.waitForFunction(() =>
    (window as WorkspaceFixture).workspaceHasStore?.(),
  );
  await page.keyboard.press("Meta+w");
  await expect(pane).toBeVisible({ timeout: 60_000 });
  await page.evaluate(() =>
    (window as WorkspaceFixture).workspaceBeginStreams(),
  );
  const scroller = pane.locator(".agent-transcript");
  await expect
    .poll(() =>
      scroller.evaluate(
        (el) =>
          el.querySelector(".transcript-turn-live")!.getBoundingClientRect()
            .height - el.clientHeight,
      ),
    )
    .toBeGreaterThan(500);
  const distance = () =>
    scroller.evaluate(
      (el) =>
        new Promise<number>((resolve) => {
          // A synchronous layout read can see newly streamed text before the
          // transcript's resize observers pin it for the next paint. Register
          // after them so this measures the position the browser will paint.
          const observer = new ResizeObserver(() => {
            observer.disconnect();
            resolve(el.scrollHeight - el.clientHeight - el.scrollTop);
          });
          observer.observe(el.querySelector("[data-transcript-content]")!);
        }),
    );
  await expect.poll(distance).toBeLessThanOrEqual(2);
  await page.keyboard.press("Meta+p");
  await page
    .getByRole("textbox", { name: "Go to File", exact: true })
    .fill("sample.ts");
  await page.getByRole("option", { name: /sample.ts/ }).click();
  await expect(page.locator(".cm-editor")).toBeVisible({ timeout: 30_000 });
  await expect.poll(distance).toBeLessThanOrEqual(2);
  const divider = page
    .locator(
      '[data-workspace-tab][aria-hidden="false"] [role="separator"][aria-valuenow]',
    )
    .first();
  const resize = async (delta: number) => {
    const box = (await divider.boundingBox())!;
    await page.mouse.move(box.x, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + delta, box.y + box.height / 2, { steps: 12 });
    await page.mouse.up();
  };
  await resize(80);
  await expect.poll(distance).toBeLessThanOrEqual(2);
  await scroller.evaluate((el) => {
    el.dispatchEvent(new WheelEvent("wheel", { deltaY: -500 }));
    el.scrollTop -= 500;
  });
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
  const anchor = await page.evaluate(() =>
    (window as WorkspaceFixture).captureReaderAnchor(),
  );
  const readerShift = () =>
    page.evaluate(
      (anchor) =>
        Math.abs(
          (window as WorkspaceFixture).readerAnchorTop(anchor) - anchor.top,
        ),
      anchor,
    );
  // Background output must not move a reader when the cached transcript's
  // DOM is detached and reattached through the real workspace shortcuts.
  await page.keyboard.press("Meta+2");
  await expect(pane.locator(".agent-transcript")).toHaveCount(0);
  await page.waitForTimeout(400);
  await page.keyboard.press("Meta+1");
  await expect(scroller).toBeVisible();
  await expect.poll(readerShift).toBeLessThanOrEqual(3);
  await resize(-140);
  await expect.poll(readerShift).toBeLessThanOrEqual(3);
  await page
    .locator('[data-workspace-tab][aria-hidden="false"]')
    .getByRole("button", { name: "Close sample.ts", exact: true })
    .click();
  await expect(page.locator(".cm-editor")).toHaveCount(0);
  await expect.poll(readerShift).toBeLessThanOrEqual(3);
  await page.getByRole("tab", { name: /^Changes/ }).click();
  await page.getByTitle("sample.ts", { exact: true }).click();
  await expect(page.locator("[data-diff-file]")).toBeVisible();
  await expect.poll(readerShift).toBeLessThanOrEqual(3);
  await page.evaluate(() =>
    (window as WorkspaceFixture).workspaceFinishStreams(),
  );
  await expect.poll(readerShift).toBeLessThanOrEqual(3);
  expect(errors).toEqual([]);
  await testInfo.attach("streaming-and-frames", {
    body: JSON.stringify(
      await page.evaluate(() =>
        (window as WorkspaceFixture).workspaceMetrics(),
      ),
    ),
    contentType: "application/json",
  });
});
