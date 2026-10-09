import { expect, test } from "@playwright/test";

const waiting = {
  tasks: ["pi-subagents"],
  busy: true,
  visible: true,
  interrupted: false,
};

test("a later background run gets a fresh timer and immediate status", async ({
  page,
}, testInfo) => {
  await page.clock.install();
  await page.goto("/tests/browser/background.html");
  const elapsed = page.locator(".background-activity-time");
  await expect(elapsed).toHaveText("0:00");
  await page.clock.fastForward(35_000);
  await expect(elapsed).toHaveText("0:35");
  await page.evaluate(
    (props) => {
      (
        window as unknown as {
          setBackgroundActivity: (value: typeof props) => void;
        }
      ).setBackgroundActivity(props);
    },
    { ...waiting, busy: false, interrupted: true, tasks: [] },
  );
  await expect(page.getByRole("status")).toHaveText("Turn stopped");
  await page.clock.fastForward(10_000);
  await page.evaluate((props) => {
    (
      window as unknown as {
        setBackgroundActivity: (value: typeof props) => void;
      }
    ).setBackgroundActivity(props);
  }, waiting);
  await expect(elapsed).toHaveText("0:00");
  await expect(page.getByRole("status")).toHaveText(
    "Background work is running",
  );
  await expect(page.locator(".background-activity-tasks")).toHaveText(
    "Delegated work",
  );
  await page.clock.fastForward(340);
  await expect(page.locator(".mono-work-ticker-row")).toHaveCount(1);
  await expect(page.locator(".mono-work-ticker-row")).toHaveText(
    "Background work is running",
  );
  await page
    .locator("main")
    .screenshot({ path: testInfo.outputPath("background-after.png") });
});

test("disclosure works with reduced motion and a narrow viewport", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/tests/browser/background.html");
  const disclosure = page.locator(".background-activity-disclosure");
  await expect(disclosure).toHaveAttribute("data-open", "true");
  await page.getByRole("button", { name: "Hide background activity" }).click();
  await expect(disclosure).toHaveAttribute("inert", "");
  await expect(disclosure).toHaveCSS("transition-duration", "0s");
  await page.getByRole("button", { name: "Show background activity" }).click();
  await expect(disclosure).not.toHaveAttribute("inert", "");
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});
