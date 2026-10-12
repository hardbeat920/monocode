import { expect, test } from "@playwright/test";

// Opt in on a controlled host: MARKDOWN_PERFORMANCE=1 npm run test:browser -- markdown-performance
const checkFrameTimes = process.env.MARKDOWN_PERFORMANCE === "1";

test("streaming a large Markdown fence keeps animations responsive", async ({
  page,
}, testInfo) => {
  test.setTimeout(60_000);
  if (checkFrameTimes && testInfo.project.name === "chromium") {
    const session = await page.context().newCDPSession(page);
    await session.send("Emulation.setCPUThrottlingRate", { rate: 4 });
  }
  await page.goto("/tests/browser/markdown-performance.html");
  await expect(page.locator(".agent-markdown")).toContainText(["Ready."]);
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => {
    const gaps: number[] = [];
    let last = performance.now();
    let frame = 0;
    let sawWordFade = false;
    const sample = (now: number) => {
      sawWordFade ||= !!document.querySelector("[data-word-fade]");
      gaps.push(now - last);
      last = now;
      frame = requestAnimationFrame(sample);
    };
    frame = requestAnimationFrame(sample);
    Object.assign(window, {
      finishMarkdownSamples: () => {
        cancelAnimationFrame(frame);
        return { gaps, sawWordFade };
      },
    });
    (
      window as unknown as { startMarkdownStream(): void }
    ).startMarkdownStream();
  });
  const code = page.locator('[data-streamdown="code-block-body"] code');
  await expect(code.locator("span span").first()).toBeVisible();
  const firstLine = await code.locator(":scope > span").first().elementHandle();
  await expect(code).toContainText("requirement79", { timeout: 60_000 });
  // Include the final highlight and the removal of the lingering word fades.
  await expect(page.locator(".word-fading")).toHaveCount(0, {
    timeout: 60_000,
  });
  await expect(code.locator("span span").first()).toBeVisible();
  const { gaps, sawWordFade } = await page.evaluate(() =>
    (
      window as unknown as {
        finishMarkdownSamples(): { gaps: number[]; sawWordFade: boolean };
      }
    ).finishMarkdownSamples(),
  );
  const sorted = [...gaps].sort((a, b) => a - b);
  const metrics = {
    frames: gaps.length,
    p95: sorted[Math.floor(sorted.length * 0.95)],
    max: Math.max(...gaps),
    over50ms: gaps.filter((gap) => gap > 50).length,
    elapsed: gaps.reduce((sum, gap) => sum + gap, 0),
  };
  await testInfo.attach("frame-times", {
    body: JSON.stringify({ metrics, gaps }),
    contentType: "application/json",
  });
  // Calibrated against HEAD with this fixture on the same host: Chromium at
  // 4x CPU throttle had ~100ms p95 / ~267-300ms max; WebKit had ~133-147ms max.
  // Fixed runs are ~33-50ms p95 in Chromium and ~35ms max in WebKit. These
  // limits allow scheduling headroom; they are a local regression check,
  // not a hardware-independent benchmark or a model-streaming simulation.
  // Shared CI scheduling can exceed these host-specific limits. Keep the
  // functional assertions below enabled regardless of the performance flag.
  if (checkFrameTimes) {
    if (testInfo.project.name === "chromium") {
      expect(metrics.p95).toBeLessThan(75);
      expect(metrics.max).toBeLessThan(150);
    } else {
      expect(metrics.max).toBeLessThan(90);
    }
  }
  expect(sawWordFade).toBe(true);
  const expected = await page.evaluate(() =>
    (window as unknown as { markdownSource: string }).markdownSource.trimEnd(),
  );
  expect(
    await code.evaluate((el) =>
      Array.from(el.children, (line) =>
        line.textContent === "\n" ? "" : line.textContent,
      )
        .join("\n")
        .trimEnd(),
    ),
  ).toBe(expected);
  expect(await firstLine?.evaluate((line) => line.isConnected)).toBe(true);

  await page.evaluate(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async (text: string) => {
          Object.assign(window, { copiedMarkdown: text });
        },
      },
    });
  });
  await page.getByRole("button", { name: "Copy code", exact: true }).click();
  expect(
    await page.evaluate(() =>
      (
        window as unknown as { copiedMarkdown: string }
      ).copiedMarkdown.trimEnd(),
    ),
  ).toBe(expected);
  const token = code.locator("span span").first();
  const darkColor = await token.evaluate((el) => getComputedStyle(el).color);
  await page.evaluate(() =>
    document.documentElement.classList.add("theme-light"),
  );
  expect(await token.evaluate((el) => getComputedStyle(el).color)).not.toBe(
    darkColor,
  );
});

test("fallback fences preserve blank lines and line numbering in both themes", async ({
  page,
}) => {
  await page.goto("/tests/browser/markdown-performance.html");
  await expect(page.locator(".agent-markdown")).toContainText(["Ready."]);
  await page.evaluate(() =>
    (
      window as unknown as { showMarkdownExample(text: string): void }
    ).showMarkdownExample(
      "```foo noLineNumbers\na\n\nb\n```\n\n```foo startLine=17\na\n\nb\n```",
    ),
  );
  const codes = page.locator('[data-streamdown="code-block-body"] code');
  await expect(codes).toHaveCount(2);
  // Wait for the async plaintext highlight, rather than testing its raw fallback.
  await expect(codes.nth(0).locator("span span")).toHaveCount(2);
  await expect(codes.nth(1).locator("span span")).toHaveCount(2);
  for (const light of [false, true]) {
    await page.evaluate(
      (light) =>
        document.documentElement.classList.toggle("theme-light", light),
      light,
    );
    for (const index of [0, 1]) {
      const layout = await codes.nth(index).evaluate((code) => {
        const line = code.children[0];
        const blank = code.children[1];
        const body = code.closest('[data-streamdown="code-block-body"]')!;
        return {
          blank: blank.textContent,
          blankHeight: blank.getBoundingClientRect().height,
          lineHeight: line.getBoundingClientRect().height,
          counterReset: getComputedStyle(code).counterReset,
          counterContent: getComputedStyle(line, "::before").content,
          whiteSpace: getComputedStyle(code.parentElement!).whiteSpace,
          fontSize: getComputedStyle(body).fontSize,
          tokenColor: getComputedStyle(line.children[0]).color,
          bodyColor: getComputedStyle(body).color,
        };
      });
      expect(layout.blank).toBe("\n");
      expect(layout.blankHeight).toBeGreaterThan(0);
      expect(layout.blankHeight).toBe(layout.lineHeight);
      expect(layout.whiteSpace).toBe("pre");
      expect(layout.fontSize).toBe("12px");
      expect(layout.tokenColor).toBe(layout.bodyColor);
      expect(layout.counterReset).toBe(index ? "line 16" : "none");
      expect(layout.counterContent).toBe(index ? "counter(line)" : "none");
    }
  }
});
