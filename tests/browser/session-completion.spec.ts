import type { ReaderAnchor } from "./readerAnchor";
import { expect, test, type Page } from "@playwright/test";
import type { Block } from "../../src/features/sessions/model/session";

type Fixture = Window & {
  captureReaderAnchor(): ReaderAnchor;
  readerAnchorTop(anchor: ReaderAnchor): number;
  setReplayBlocks(blocks: Block[], busy: boolean): void;
};

// Exercise a long reply with headings, nested lists, inline file links and a
// short code fence across streaming and completion renders.
const reply =
  Array.from({ length: 10 }, (_, index) => {
    const section = `## Step ${index + 1} — Prepare the application

Check the project settings before continuing. The sample application uses a shared configuration library, and the generated project must use the latest configuration from \`app.json\`.

1. **Review the configuration.** Open the project and check the selected destination, application identifier, and signing options before continuing with the release.
2. **Check the generated files.** Keep these details together:
   - The version and build number identify this upload.
   - The release configuration includes the native connection library.
3. **Continue when ready.** Inspect the result and keep the conversation open while the next operation finishes.`;
    return index === 3
      ? `${section}

\`\`\`bash
npm run build
npm run preview
\`\`\``
      : section;
  }).join("\n\n") + "\n\nAll preparation steps are complete.";

const prompt: Block = {
  id: "latest-prompt",
  role: "user",
  text: "Give me the detailed steps.",
  startedAt: Date.now(),
};
const head: Block[] = [
  ...Array.from({ length: 30 }, (_, i): Block[] => [
    { id: `user-${i}`, role: "user", text: `Earlier question ${i}` },
    {
      id: `reply-${i}`,
      role: "assistant",
      text: "Earlier answer. ".repeat(30),
    },
  ]).flat(),
  prompt,
  {
    id: "tool",
    role: "tool",
    text: "cat app.json",
    tool: { kind: "execute", status: "completed" },
  },
];

async function openPane(page: Page, width: number) {
  await page.goto("/tests/browser/session-pane.html");
  await page.waitForFunction(
    () => typeof (window as Fixture).setReplayBlocks === "function",
  );
  await page.evaluate(
    ({ head, width }) => {
      document.getElementById("root")!.style.width = `${width}px`;
      (window as Fixture).setReplayBlocks(head, true);
    },
    { head, width },
  );
  await expect(
    page.locator('[data-chat-message="latest-prompt"]'),
  ).toBeVisible();
  // Historical turns mount progressively. Finish opening the fixture before
  // measuring completion so unrelated history growth is not in the sample.
  await expect(page.locator("[data-transcript-turn]")).toHaveCount(20);
  await page.evaluate(() => document.fonts.ready);
  await expect
    .poll(() =>
      page
        .locator(".agent-transcript")
        .evaluate((el) => el.scrollHeight - el.clientHeight - el.scrollTop),
    )
    .toBeLessThanOrEqual(1);
}

async function completeAndRecord(
  page: Page,
  incremental: boolean,
  reading = false,
) {
  return page.evaluate(
    async ({ head, reply, incremental, reading }) => {
      const fixture = window as Fixture;
      const el = document.querySelector<HTMLElement>(".agent-transcript")!;
      const anchor = reading ? fixture.captureReaderAnchor() : null;
      const content = el.querySelector<HTMLElement>(
        "[data-transcript-content]",
      );
      const scrollWrites: Array<{
        from: number;
        requested: number;
        actual: number;
        transform: string;
        stack?: string;
      }> = [];
      const scrollTop = Object.getOwnPropertyDescriptor(
        Element.prototype,
        "scrollTop",
      )!;
      if (reading)
        Object.defineProperty(el, "scrollTop", {
          configurable: true,
          get: () => scrollTop.get!.call(el),
          set: (value: number) => {
            const from = scrollTop.get!.call(el);
            scrollTop.set!.call(el, value);
            if (from !== scrollTop.get!.call(el))
              scrollWrites.push({
                from,
                requested: value,
                actual: scrollTop.get!.call(el),
                transform: content?.style.transform ?? "",
                stack: new Error().stack?.split("\n").slice(1, 6).join("\n"),
              });
          },
        });
      const samples: Array<{
        top: number;
        readerTop: number | null;
        height: number;
        viewport: number;
        length: number;
        fading: boolean;
        transform: string;
      }> = [];
      let frame = 0;
      const read = () => {
        const output = el.querySelector('[data-chat-message="live-reply"]');
        samples.push({
          top: el.scrollTop,
          readerTop: anchor ? fixture.readerAnchorTop(anchor) : null,
          height: el.scrollHeight,
          viewport: el.clientHeight,
          length: output?.textContent?.length ?? 0,
          fading: !!output?.querySelector(".word-fading"),
          transform: content?.style.transform ?? "",
        });
        frame = requestAnimationFrame(read);
      };
      read();
      if (incremental) {
        for (let end = 200; end < reply.length; end += 200) {
          fixture.setReplayBlocks(
            [
              ...head,
              {
                id: "live-reply",
                role: "assistant",
                text: reply.slice(0, end),
                streaming: true,
              },
            ],
            true,
          );
          await new Promise((resolve) => setTimeout(resolve, 50));
        }
      }
      fixture.setReplayBlocks(
        [
          ...head.map((block) =>
            block.id === "latest-prompt"
              ? { ...block, durationMs: 28000 }
              : block,
          ),
          { id: "live-reply", role: "assistant", text: reply },
        ],
        false,
      );
      const deadline = performance.now() + 5000;
      let settledAt = 0;
      do {
        await new Promise(requestAnimationFrame);
        const output = el.querySelector('[data-chat-message="live-reply"]');
        if (
          !settledAt &&
          output?.textContent?.endsWith(
            "All preparation steps are complete.",
          ) &&
          !output.querySelector(".word-fading")
        )
          settledAt = performance.now();
      } while (
        performance.now() < deadline &&
        (!settledAt || performance.now() < settledAt + 300)
      );
      cancelAnimationFrame(frame);
      if (reading) Reflect.deleteProperty(el, "scrollTop");
      return { samples, scrollWrites, anchor, settled: !!settledAt };
    },
    { head, reply, incremental, reading },
  );
}

for (const width of [650, 1000]) {
  for (const incremental of [false, true]) {
    test(`full session pane follows ${incremental ? "streamed" : "burst"} completion at ${width}px without reversing`, async ({
      page,
    }, info) => {
      const errors: string[] = [];
      page.on("pageerror", (error) => errors.push(error.message));
      page.on("console", (message) => {
        if (message.type() === "error") errors.push(message.text());
      });
      await openPane(page, width);
      const result = await completeAndRecord(page, incremental);
      await info.attach("completion-scroll-frames", {
        body: JSON.stringify(result),
        contentType: "application/json",
      });
      expect(errors).toEqual([]);
      expect(result.settled).toBe(true);
      expect(
        result.samples
          .slice(1)
          .filter((sample, i) => sample.top < result.samples[i].top - 1),
      ).toEqual([]);
      const last = result.samples.at(-1)!;
      expect(last.height - last.viewport - last.top).toBeLessThanOrEqual(1);
      await expect(
        page.getByRole("toolbar", { name: "Prompts", includeHidden: true }),
      ).toBeVisible({ visible: width >= 928 });
    });
  }
}

test("completion preserves a reader's position inside the full session pane", async ({
  page,
}, info) => {
  await openPane(page, 850);
  await page.evaluate(
    ({ head, reply }) =>
      (window as Fixture).setReplayBlocks(
        [
          ...head,
          { id: "live-reply", role: "assistant", text: reply, streaming: true },
        ],
        true,
      ),
    { head, reply },
  );
  await expect(page.locator('[data-chat-message="live-reply"]')).toContainText(
    "All preparation steps are complete.",
  );
  const scroller = page.locator(".agent-transcript");
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
  const result = await completeAndRecord(page, false, true);
  await info.attach("completion-reading-frames", {
    body: JSON.stringify(result),
    contentType: "application/json",
  });
  expect(result.settled).toBe(true);
  const tops = result.samples.map((sample) => sample.readerTop!);
  expect(Math.max(...tops) - Math.min(...tops)).toBeLessThanOrEqual(1);
});
