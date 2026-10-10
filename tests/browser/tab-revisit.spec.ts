import { expect, test, type Page } from "@playwright/test";

type Reply = { text: string; streaming: boolean; busy: boolean };
type Fixture = Window & {
  REPLY: string;
  setReply: (reply: Reply) => void;
  hideTab: () => void;
  showTab: () => void;
};

const frames = (page: Page, count: number) =>
  page.evaluate(
    (count) =>
      new Promise<void>((resolve) => {
        let left = count;
        const tick = () =>
          --left <= 0 ? resolve() : requestAnimationFrame(tick);
        requestAnimationFrame(tick);
      }),
    count,
  );

const setReply = (page: Page, reply: Reply) =>
  page.evaluate(
    (reply) => (window as unknown as Fixture).setReply(reply),
    reply,
  );

async function streamWords(page: Page, from: number, to: number) {
  const words = await page.evaluate(() =>
    (window as unknown as Fixture).REPLY.split(" "),
  );
  for (let i = from; i <= Math.min(to, words.length); i += 3) {
    await setReply(page, {
      text: words.slice(0, i).join(" "),
      streaming: true,
      busy: true,
    });
    await frames(page, 1);
  }
  return words;
}

/** Word fades running inside the reply, a few frames after the tab shows. */
async function fadesAfterShowing(page: Page) {
  await page.evaluate(() => (window as unknown as Fixture).showTab());
  await frames(page, 3);
  return page.evaluate(
    () =>
      document
        .getAnimations()
        .filter(
          (animation) =>
            animation.playState === "running" &&
            (animation as CSSAnimation).animationName === "word-fade-in",
        ).length,
  );
}

test("a reply hidden mid-fade does not fade in again when its tab returns", async ({
  page,
}) => {
  await page.goto("/tests/browser/tab-revisit.html");
  await streamWords(page, 1, Infinity);
  await page.evaluate(() => (window as unknown as Fixture).hideTab());
  await page.evaluate(() => {
    const fixture = window as unknown as Fixture;
    fixture.setReply({ text: fixture.REPLY, streaming: false, busy: false });
  });
  await page.waitForTimeout(1000);

  const reply = page.locator('[data-chat-message="reply-1"]');
  await expect(reply.locator("[data-word-fade]").first()).toBeAttached();
  expect(await fadesAfterShowing(page)).toBe(0);
});

test("words that arrive after the tab returns still fade in", async ({
  page,
}) => {
  await page.goto("/tests/browser/tab-revisit.html");
  const words = await streamWords(page, 1, 30);
  await page.evaluate(() => (window as unknown as Fixture).hideTab());
  await page.waitForTimeout(500);
  expect(await fadesAfterShowing(page)).toBe(0);

  await setReply(page, {
    text: words.slice(0, 45).join(" "),
    streaming: true,
    busy: true,
  });
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          document
            .getAnimations()
            .filter(
              (animation) =>
                (animation as CSSAnimation).animationName === "word-fade-in",
            ).length,
      ),
    )
    .toBeGreaterThan(0);
  const settled = await page
    .locator('[data-chat-message="reply-1"] [data-word-settled]')
    .count();
  expect(settled).toBeGreaterThanOrEqual(25);
});
