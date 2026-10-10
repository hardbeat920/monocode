import { expect, test } from "@playwright/test";

test("file preview decodes, plays and seeks a video, then pauses on tab change", async ({
  page,
}) => {
  await page.goto("/tests/browser/video-preview.html");
  const video = page.locator('video[aria-label="Video preview: clip.mp4"]');
  await expect(video).toBeVisible();
  await expect
    .poll(
      () =>
        video.evaluate((element: HTMLVideoElement) => ({
          metadataReady: element.readyState >= HTMLMediaElement.HAVE_METADATA,
          error: element.error
            ? { code: element.error.code, message: element.error.message }
            : null,
          networkState: element.networkState,
        })),
      // Allow the cold WebKit media pipeline to initialize on shared runners.
      {
        timeout: 15_000,
        message: "Video should load metadata without a media error",
      },
    )
    .toMatchObject({ metadataReady: true, error: null });
  await expect(page.locator("footer").first()).toContainText("160 × 90");
  await expect(page.locator("footer").first()).toContainText("0:03");

  await video.evaluate(async (element: HTMLVideoElement) => {
    element.muted = true;
    await element.play();
  });
  await expect
    .poll(() =>
      video.evaluate((element: HTMLVideoElement) => element.currentTime),
    )
    .toBeGreaterThan(0);
  await video.evaluate((element: HTMLVideoElement) => {
    element.currentTime = 1.5;
  });
  await expect
    .poll(() =>
      video.evaluate((element: HTMLVideoElement) => element.currentTime),
    )
    .toBeGreaterThanOrEqual(1.5);

  await page.getByText("other.mp4", { exact: true }).click();
  await expect(video).toBeHidden();
  await expect
    .poll(() => video.evaluate((element: HTMLVideoElement) => element.paused))
    .toBe(true);
  await page.getByText("clip.mp4", { exact: true }).click();
  await expect(video).toBeVisible();
  expect(
    await video.evaluate((element: HTMLVideoElement) => element.paused),
  ).toBe(true);

  const bounds = await video.boundingBox();
  expect(bounds!.width).toBeLessThanOrEqual(850);
  expect(bounds!.height).toBeLessThanOrEqual(600);

  await video.evaluate(async (element: HTMLVideoElement) => {
    await element.play();
  });
  await page.getByRole("button", { name: "Hide workspace" }).click();
  await expect(video).toBeHidden();
  await expect
    .poll(() => video.evaluate((element: HTMLVideoElement) => element.paused))
    .toBe(true);
  await page.getByRole("button", { name: "Show workspace" }).click();
  await expect(video).toBeVisible();
  expect(
    await video.evaluate((element: HTMLVideoElement) => element.paused),
  ).toBe(true);
  await page.screenshot({ path: test.info().outputPath("video-preview.png") });
});
