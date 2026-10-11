import { expect, test } from "@playwright/test";

type MonoScrollWindow = Window & {
  completeMonoReply: (text?: string) => void;
  finishMonoSamples: () => {
    at: number;
    height: number;
    offset: number;
    top: number;
  }[];
};

type MonoCompletionWindow = Window & {
  queueMonoCompletion: () => void;
  beginMonoCompletion: () => void;
  finishMonoCompletion: () => void;
  startCompletionSamples: () => void;
  finishCompletionSamples: () => {
    cardTop: number;
    height: number;
    viewport: number;
  }[];
};

for (const length of ["short", "scrolling"]) {
  test(`a Mono completion attaches without reversing the ${length} chat's motion`, async ({
    page,
  }) => {
    await page.goto(
      `/tests/browser/transcript.html?monoCompletion${length === "short" ? "&short" : ""}`,
    );
    const scroller = page.locator(".agent-transcript");
    const card = scroller.locator("[data-launched-session]");
    const content = scroller.locator("[data-transcript-content]");
    await expect(card).toHaveAttribute("data-state", "working");
    await page.evaluate(() => document.fonts.ready);
    await expect(content).toHaveCSS("transform", "none");
    await page.evaluate(() => {
      const el = document.querySelector<HTMLElement>(".agent-transcript")!;
      const card = el.querySelector<HTMLElement>("[data-launched-session]")!;
      const read = () => ({
        cardTop: card.getBoundingClientRect().top,
        height: el.scrollHeight,
        viewport: el.clientHeight,
      });
      let samples: ReturnType<typeof read>[] = [];
      let frame = 0;
      const sample = () => {
        samples.push(read());
        frame = requestAnimationFrame(sample);
      };
      Object.assign(window, {
        startCompletionSamples: () => {
          samples = [read()];
          frame = requestAnimationFrame(sample);
        },
        finishCompletionSamples: () => {
          cancelAnimationFrame(frame);
          return samples;
        },
      });
      (window as MonoCompletionWindow).startCompletionSamples();
      (window as MonoCompletionWindow).queueMonoCompletion();
    });
    // Delivery can await provider preflight after queuing. Capture that
    // intermediate paint too, before the hidden prompt reaches the Mono.
    await page.waitForTimeout(120);
    await page.evaluate(() =>
      (window as MonoCompletionWindow).beginMonoCompletion(),
    );
    await expect(card).toHaveAttribute("data-state", "done");
    // Let a temporary height correction complete; a final-position-only
    // assertion would miss the visible dip before the report is attached.
    await page.waitForTimeout(500);
    const reopened = await page.evaluate(() =>
      (window as MonoCompletionWindow).finishCompletionSamples(),
    );
    const tops = reopened.map((sample) => sample.cardTop);
    expect(Math.max(...tops) - Math.min(...tops)).toBeLessThanOrEqual(1);
    const viewports = reopened.map((sample) => sample.viewport);
    expect(Math.max(...viewports) - Math.min(...viewports)).toBeLessThanOrEqual(
      1,
    );

    await page.evaluate(() => {
      (window as MonoCompletionWindow).startCompletionSamples();
      (window as MonoCompletionWindow).finishMonoCompletion();
    });
    await expect(
      scroller.locator('[data-chat-message="mono-report"]'),
    ).toBeVisible();
    await expect(content).toHaveCSS("transform", "none");
    const attached = await page.evaluate(() =>
      (window as MonoCompletionWindow).finishCompletionSamples(),
    );
    const reversals = attached
      .slice(1)
      .filter((sample, index) => sample.cardTop > attached[index].cardTop + 1);
    expect(reversals).toEqual([]);
    expect(attached[0].cardTop - attached.at(-1)!.cardTop).toBeGreaterThan(100);
    await expect(scroller.locator("[data-transcript-turn]")).toHaveCount(
      length === "short" ? 1 : 7,
    );
    await expect(scroller.locator("[data-launched-session]")).toHaveCount(1);
  });
}

test("a completed Mono reply glides through its full height", async ({
  page,
}) => {
  await page.goto("/tests/browser/transcript.html?mono");
  const scroller = page.locator(".agent-transcript");
  await expect(scroller.locator(".transcript-turn")).toHaveCount(7);
  await page.evaluate(() => document.fonts.ready);
  await expect
    .poll(() =>
      scroller.evaluate(
        (el) => el.scrollHeight - el.clientHeight - el.scrollTop,
      ),
    )
    .toBeLessThanOrEqual(1);

  await page.evaluate(() => {
    const el = document.querySelector<HTMLElement>(".agent-transcript")!;
    const content = el.querySelector<HTMLElement>("[data-transcript-content]")!;
    const read = () => {
      const offset = Number.parseFloat(
        content.style.transform.match(/translateY\((.*)px\)/)?.[1] ?? "0",
      );
      return {
        at: performance.now(),
        height: el.scrollHeight,
        offset,
        top: el.scrollTop - offset,
      };
    };
    const samples = [read()];
    let frame = 0;
    const sample = () => {
      samples.push(read());
      frame = requestAnimationFrame(sample);
    };
    frame = requestAnimationFrame(sample);
    Object.assign(window, {
      finishMonoSamples: () => {
        cancelAnimationFrame(frame);
        return samples;
      },
    });
    (window as MonoScrollWindow).completeMonoReply();
  });
  await expect(
    scroller.locator('[data-chat-message="mono-answer"]'),
  ).toBeVisible();
  await expect(scroller.locator("[data-transcript-content]")).toHaveCSS(
    "transform",
    "none",
  );
  const samples = await page.evaluate(() =>
    (window as MonoScrollWindow).finishMonoSamples(),
  );
  const growth = samples.at(-1)!.height - samples[0].height;
  const moving = samples.filter((sample) => sample.offset > 1);
  expect(growth).toBeGreaterThan(500);
  // Preserve the reader's place when the entire answer first becomes visible.
  expect(Math.max(...samples.map((sample) => sample.offset))).toBeGreaterThan(
    growth - 40,
  );
  expect(moving.at(-1)!.at - moving[0].at).toBeGreaterThan(300);
  const steps = samples
    .slice(1)
    .map((sample, index) => sample.top - samples[index].top);
  expect(Math.max(...steps)).toBeLessThan(growth / 3);
  expect(Math.min(...steps)).toBeGreaterThanOrEqual(-2);
  expect(
    await scroller.evaluate(
      (el) => el.scrollHeight - el.clientHeight - el.scrollTop,
    ),
  ).toBeLessThanOrEqual(1);
});

test("scrolling up interrupts a completed Mono reply's glide", async ({
  page,
}) => {
  await page.goto("/tests/browser/transcript.html?mono");
  const scroller = page.locator(".agent-transcript");
  const content = scroller.locator("[data-transcript-content]");
  await expect(scroller.locator(".transcript-turn")).toHaveCount(7);
  await page.evaluate(() => document.fonts.ready);
  await scroller.hover();
  await page.evaluate(() => (window as MonoScrollWindow).completeMonoReply());
  await expect(
    scroller.locator('[data-chat-message="mono-answer"]'),
  ).toBeVisible();
  await page.mouse.wheel(0, -80);
  await expect(content).toHaveCSS("transform", "none");
  await expect
    .poll(() =>
      scroller.evaluate(
        (el) => el.scrollHeight - el.clientHeight - el.scrollTop,
      ),
    )
    .toBeGreaterThan(100);
  const top = await scroller.evaluate((el) => el.scrollTop);
  await page.evaluate(() =>
    (window as MonoScrollWindow).completeMonoReply("More text. ".repeat(600)),
  );
  await expect(
    scroller.locator('[data-chat-message="mono-answer"]'),
  ).toContainText("More text.");
  expect(await scroller.evaluate((el) => el.scrollTop)).toBeCloseTo(top, 0);
});

test("scrolling settled history keeps the scroll range fixed", async ({
  page,
}) => {
  await page.goto("/tests/browser/transcript.html");
  const scroller = page.locator(".agent-transcript");
  await expect(scroller.locator(".transcript-turn")).toHaveCount(20);
  await page.evaluate(() => document.fonts.ready);
  await expect
    .poll(() =>
      scroller.evaluate(
        (el) => el.scrollHeight - el.clientHeight - el.scrollTop,
      ),
    )
    .toBeLessThanOrEqual(1);

  // Sample actual browser layout every frame. Checking only the final height
  // could miss a correction that settles before the gesture ends.
  const initial = await scroller.evaluate((el) => {
    const samples = [{ height: el.scrollHeight, top: el.scrollTop }];
    let frame = 0;
    const sample = () => {
      samples.push({ height: el.scrollHeight, top: el.scrollTop });
      frame = requestAnimationFrame(sample);
    };
    frame = requestAnimationFrame(sample);
    Object.assign(window, {
      finishScrollSamples: () => {
        cancelAnimationFrame(frame);
        return samples;
      },
    });
    return samples[0];
  });
  expect(initial.top).toBeGreaterThan(3000);
  await scroller.hover();
  for (let tick = 0; tick < 40; tick++) {
    await page.mouse.wheel(0, -160);
    await page.evaluate(
      () =>
        new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        ),
    );
  }
  const samples = await page.evaluate(() =>
    (
      window as unknown as {
        finishScrollSamples: () => { height: number; top: number }[];
      }
    ).finishScrollSamples(),
  );
  const heights = samples.map((sample) => sample.height);
  expect(Math.max(...heights) - Math.min(...heights)).toBeLessThanOrEqual(1);
  expect(initial.top - samples[samples.length - 1].top).toBeGreaterThan(3000);
  const upwardGestureReversals = samples
    .slice(1)
    .filter((sample, index) => sample.top > samples[index].top + 1);
  expect(upwardGestureReversals).toEqual([]);
});

test("revisiting a parked transcript returns to the latest turn", async ({
  page,
}) => {
  await page.goto("/tests/browser/transcript.html");
  const scroller = page.locator(".agent-transcript");
  const distanceFromBottom = () =>
    scroller.evaluate((el) => el.scrollHeight - el.clientHeight - el.scrollTop);
  await expect(scroller.locator(".transcript-turn")).toHaveCount(20);
  await expect.poll(distanceFromBottom).toBeLessThanOrEqual(1);

  await page.evaluate(() =>
    (window as unknown as { parkTranscript: () => void }).parkTranscript(),
  );
  await expect(scroller).toHaveCount(0);
  await page.evaluate(() =>
    (window as unknown as { showTranscript: () => void }).showTranscript(),
  );

  await expect.poll(distanceFromBottom).toBeLessThanOrEqual(1);
  await page.waitForTimeout(300);
  expect(await distanceFromBottom()).toBeLessThanOrEqual(1);
});
