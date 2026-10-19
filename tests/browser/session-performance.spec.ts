import type { ReaderAnchor } from "./readerAnchor";
import { expect, test } from "@playwright/test";

type Fixture = Window & {
  captureReaderAnchor(): ReaderAnchor;
  readerAnchorTop(anchor: ReaderAnchor): number;
  selectSession(id: number): void;
  deliverReply(text: string, complete?: boolean): void;
  sessionBurst: string;
  formattedReply: string;
  bulletReply: string;
  transcriptPoolLimit: number;
  transcriptPool: { getSnapshot(): { id: string }[] };
};

for (const example of ["formattedReply", "bulletReply"] as const) {
  for (const reading of [false, true]) {
    test(`completion keeps ${example} stable while ${reading ? "reading above the bottom" : "following the reply"}`, async ({
      page,
    }, testInfo) => {
      await page.goto("/tests/browser/session-performance.html");
      const scroller = page.locator(".agent-transcript");
      const reply = scroller.locator('[data-chat-message="live-reply"]');
      await expect(scroller.locator("[data-transcript-turn]")).toHaveCount(20);
      await page.evaluate(() => document.fonts.ready);
      await page.evaluate((example) => {
        const fixture = window as Fixture;
        fixture.deliverReply(fixture[example]);
      }, example);
      await expect(reply).toContainText(
        example === "formattedReply"
          ? "All preparation steps are complete."
          : "All checks are complete.",
      );
      await expect
        .poll(() =>
          scroller.evaluate(
            (el) => el.scrollHeight - el.clientHeight - el.scrollTop,
          ),
        )
        .toBeLessThanOrEqual(1);
      if (reading) {
        await scroller.hover();
        await page.mouse.wheel(0, -300);
        await expect
          .poll(() =>
            scroller.evaluate(
              (el) => el.scrollHeight - el.clientHeight - el.scrollTop,
            ),
          )
          .toBeGreaterThan(100);
        await expect
          .poll(() =>
            scroller.evaluate(
              (el) =>
                new Promise<boolean>((resolve) => {
                  const top = el.scrollTop;
                  setTimeout(() => resolve(el.scrollTop === top), 150);
                }),
            ),
          )
          .toBe(true);
      }
      const result = await page.evaluate(
        async ({ example, reading }) => {
          const fixture = window as Fixture;
          const el = document.querySelector<HTMLElement>(".agent-transcript")!;
          const reply = el.querySelector<HTMLElement>(
            '[data-chat-message="live-reply"]',
          )!;
          const list = reply.querySelector("ol, ul")!;
          const code = reply.querySelector(".markdown-code-shell");
          const anchor = reading ? fixture.captureReaderAnchor() : null;
          const read = () => ({
            top: el.scrollTop,
            height: el.scrollHeight,
            replyHeight: reply.getBoundingClientRect().height,
            readerTop: anchor ? fixture.readerAnchorTop(anchor) : null,
            codeHeight: code?.getBoundingClientRect().height,
            fading: !!reply.querySelector(".word-fading"),
          });
          const samples = [read()];
          fixture.deliverReply(fixture[example], true);
          const deadline = performance.now() + 900;
          do {
            await new Promise(requestAnimationFrame);
            samples.push(read());
          } while (performance.now() < deadline);
          return {
            samples,
            listPreserved: reply.querySelector("ol, ul") === list,
            codePreserved: reply.querySelector(".markdown-code-shell") === code,
            wordSpans: reply.querySelectorAll("[data-word-fade]").length,
          };
        },
        { example, reading },
      );
      await testInfo.attach("completion-scroll-frames", {
        body: JSON.stringify(result.samples),
        contentType: "application/json",
      });
      expect(result.listPreserved).toBe(true);
      expect(result.codePreserved).toBe(true);
      expect(result.wordSpans).toBe(0);
      expect(result.samples.at(-1)!.fading).toBe(false);
      // Final-position assertions miss a collapse followed by a correction.
      const heights = result.samples.map((sample) => sample.replyHeight);
      expect(Math.max(...heights) - Math.min(...heights)).toBeLessThanOrEqual(
        1,
      );
      const tops = result.samples.map((sample) => sample.top);
      if (reading) {
        // A status row above the reply may change height on completion. The
        // scroll offset must compensate while the text stays in place.
        const positions = result.samples.map((sample) => sample.readerTop!);
        expect(
          Math.max(...positions) - Math.min(...positions),
        ).toBeLessThanOrEqual(1);
      } else {
        expect(tops.slice(1).filter((top, i) => top < tops[i] - 1)).toEqual([]);
        await expect
          .poll(() =>
            scroller.evaluate(
              (el) => el.scrollHeight - el.clientHeight - el.scrollTop,
            ),
          )
          .toBeLessThanOrEqual(1);
      }
    });
  }
}

test("prompt anchoring survives a completed run and a pooled hotkey revisit", async ({
  page,
}) => {
  await page.goto("/tests/browser/session-performance.html");
  const scroller = page.locator(".agent-transcript");
  const turn = page.locator('[data-transcript-turn="latest-prompt"]');
  await expect(scroller.locator("[data-transcript-turn]")).toHaveCount(20);
  await page.evaluate(() => document.fonts.ready);
  const offset = () =>
    turn.evaluate(
      (el) =>
        el.getBoundingClientRect().top -
        el.closest(".agent-transcript")!.getBoundingClientRect().top,
    );
  await expect.poll(offset).toBeLessThanOrEqual(1);
  const mounted = await scroller.elementHandle();
  await page.evaluate(() =>
    (window as Fixture).deliverReply("A short completed answer.", true),
  );
  await expect(page.locator(".word-fading")).toHaveCount(0);
  await expect.poll(offset).toBeLessThanOrEqual(1);
  await page.keyboard.press("1");
  await expect(page.locator("[data-session]")).toHaveAttribute(
    "data-session",
    "1",
  );
  await page.keyboard.press("0");
  await expect(page.locator("[data-session]")).toHaveAttribute(
    "data-session",
    "0",
  );
  expect(await mounted!.evaluate((el) => el.isConnected)).toBe(true);
  await expect(turn).toHaveClass(/transcript-turn-anchor/);
  await expect.poll(offset).toBeLessThanOrEqual(1);
  // Check both directions; a negative offset would hide the prompt above us.
  expect(await offset()).toBeGreaterThanOrEqual(-1);
  await page.evaluate(() => {
    document.getElementById("root")!.style.height = "480px";
  });
  await expect.poll(offset).toBeLessThanOrEqual(1);
  expect(await offset()).toBeGreaterThanOrEqual(-1);
});

test("paces a completed provider burst and follows it beyond the viewport", async ({
  page,
}) => {
  await page.goto("/tests/browser/session-performance.html");
  await expect(
    page.locator('[data-transcript-turn="latest-prompt"]'),
  ).toBeVisible();
  const samples = await page.evaluate(async () => {
    const fixture = window as Fixture;
    fixture.deliverReply(fixture.sessionBurst, true);
    const lengths: number[] = [];
    const distinct = new Set<number>();
    // A busy runner can spend most of a short window rendering the first
    // chunk. Observe the required progression instead of assuming a frame rate.
    const deadline = performance.now() + 5_000;
    while (distinct.size < 4 && performance.now() < deadline) {
      await new Promise(requestAnimationFrame);
      const length =
        document.querySelector('[data-chat-message="live-reply"]')?.textContent
          ?.length ?? 0;
      lengths.push(length);
      distinct.add(length);
    }
    return { lengths, total: fixture.sessionBurst.length };
  });
  expect(samples.lengths.some((n) => n > 0 && n < samples.total / 2)).toBe(
    true,
  );
  expect(new Set(samples.lengths).size).toBeGreaterThan(3);
  await expect(page.locator('[data-chat-message="live-reply"]')).toContainText(
    "Paragraph 23.",
  );
  await expect(page.locator(".word-fading")).toHaveCount(0);
  const scroller = page.locator(".agent-transcript");
  await expect
    .poll(() =>
      scroller.evaluate(
        (el) => el.scrollHeight - el.clientHeight - el.scrollTop,
      ),
    )
    .toBeLessThanOrEqual(1);
  // A wheel gesture releases following even while more output arrives.
  await scroller.hover();
  await page.mouse.wheel(0, -300);
  await expect
    .poll(() =>
      scroller.evaluate(
        (el) => el.scrollHeight - el.clientHeight - el.scrollTop,
      ),
    )
    .toBeGreaterThan(100);
  await expect
    .poll(() =>
      scroller.evaluate(
        (el) =>
          new Promise<boolean>((resolve) => {
            const top = el.scrollTop;
            setTimeout(() => resolve(el.scrollTop === top), 150);
          }),
      ),
    )
    .toBe(true);
  const anchor = await page.evaluate(() =>
    (window as Fixture).captureReaderAnchor(),
  );
  await page.evaluate(() =>
    (window as Fixture).deliverReply(
      (window as Fixture).sessionBurst + "\n\nOne more paragraph.",
      true,
    ),
  );
  await expect(page.locator('[data-chat-message="live-reply"]')).toContainText(
    "One more paragraph.",
  );
  await expect
    .poll(() =>
      page.evaluate(
        (anchor) =>
          Math.abs((window as Fixture).readerAnchorTop(anchor) - anchor.top),
        anchor,
      ),
    )
    .toBeLessThanOrEqual(1);
});

test("reuses warm transcripts within the cache limit on rapid keyboard switches", async ({
  page,
}, testInfo) => {
  if (
    process.env.MARKDOWN_PERFORMANCE === "1" &&
    testInfo.project.name === "chromium"
  ) {
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setCPUThrottlingRate", { rate: 4 });
  }
  await page.goto("/tests/browser/session-performance.html");
  await expect(page.locator(".agent-transcript")).toBeVisible();
  await page.waitForFunction(
    () => typeof (window as Fixture).transcriptPoolLimit === "number",
  );
  const warmCount = await page.evaluate(
    () => (window as Fixture).transcriptPoolLimit,
  );
  expect(warmCount).toBeGreaterThan(1);
  expect(warmCount).toBeLessThan(10);
  const nodes = [];
  for (let id = 0; id < warmCount; id++) {
    await page.keyboard.press(String(id));
    await expect(page.locator("[data-session]")).toHaveAttribute(
      "data-session",
      String(id),
    );
    await expect(page.locator("[data-transcript-turn]")).toHaveCount(20);
    nodes.push(await page.locator(".agent-transcript").elementHandle());
  }
  const latencies: number[] = [];
  for (let id = 0; id < warmCount; id++) {
    latencies.push(
      await page.evaluate(async (id) => {
        const start = performance.now();
        window.dispatchEvent(new KeyboardEvent("keydown", { key: String(id) }));
        do {
          await new Promise(requestAnimationFrame);
        } while (
          document
            .querySelector("[data-session]")
            ?.getAttribute("data-session") !== String(id)
        );
        return performance.now() - start;
      }, id),
    );
    expect(await nodes[id]!.evaluate((el) => el.isConnected)).toBe(true);
  }
  await testInfo.attach("warm-switch-latencies", {
    body: JSON.stringify(latencies),
    contentType: "application/json",
  });
  if (process.env.MARKDOWN_PERFORMANCE === "1")
    expect(Math.max(...latencies)).toBeLessThan(150);
});

test("restores an evicted transcript with its conversation intact", async ({
  page,
}) => {
  await page.goto("/tests/browser/session-performance.html");
  const scroller = page.locator(".agent-transcript");
  await expect(scroller).toBeVisible();
  await page.evaluate(() =>
    (window as Fixture).deliverReply(
      "The reply survives cache eviction.",
      true,
    ),
  );
  const reply = page.locator('[data-chat-message="live-reply"]');
  await expect(reply).toHaveText("The reply survives cache eviction.");
  await expect(page.locator("[data-transcript-turn]")).toHaveCount(20);
  const original = await scroller.elementHandle();
  await page.waitForFunction(
    () => typeof (window as Fixture).transcriptPoolLimit === "number",
  );
  const limit = await page.evaluate(
    () => (window as Fixture).transcriptPoolLimit,
  );
  for (let id = 1; id <= limit + 1; id++) {
    await page.keyboard.press(String(id));
    await expect(page.locator("[data-session]")).toHaveAttribute(
      "data-session",
      String(id),
    );
    await expect(page.locator("[data-transcript-turn]")).toHaveCount(20);
  }
  const retained = await page.evaluate(() =>
    (window as Fixture).transcriptPool.getSnapshot().map((entry) => entry.id),
  );
  expect(retained).not.toContain("session-0");
  await page.keyboard.press("0");
  await expect(page.locator("[data-session]")).toHaveAttribute(
    "data-session",
    "0",
  );
  await expect(page.locator("[data-transcript-turn]")).toHaveCount(20);
  await expect(reply).toHaveText("The reply survives cache eviction.");
  expect(await original!.evaluate((el) => el.isConnected)).toBe(false);
});
