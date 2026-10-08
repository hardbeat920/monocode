import { expect, test } from "@playwright/test";

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
