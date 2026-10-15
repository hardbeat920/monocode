import { expect, test, type Page } from "@playwright/test";

async function showExample(page: Page, text: string, streaming = false) {
  await page.evaluate(
    ({ text, streaming }) =>
      (
        window as unknown as {
          showMarkdownExample(text: string, streaming: boolean): void;
        }
      ).showMarkdownExample(text, streaming),
    { text, streaming },
  );
}

test.beforeEach(async ({ page }) => {
  await page.goto("/tests/browser/markdown-performance.html");
  await expect(page.locator(".agent-markdown")).toContainText("Ready.");
  await page.evaluate(() => {
    document.getElementById("root")!.style.width = "320px";
  });
});

test("toggle changes actual wrapping independently and preserves copied source", async ({
  page,
}) => {
  const source = `\t${"A long Markdown paragraph. ".repeat(24)}\n\nhttps://example.com/${"segment".repeat(60)}\nlast line  `;
  await showExample(
    page,
    `\`\`\`markdown\n${source}\n\`\`\`\n\n\`\`\`typescript\nconst x = 1;\n\`\`\``,
  );
  const blocks = page.locator(".markdown-code-shell");
  const toggle = blocks.first().getByRole("button", { name: "Wrap lines" });
  const body = blocks.first().locator('[data-streamdown="code-block-body"]');
  const overflow = () =>
    body.evaluate((el) => el.scrollWidth > el.clientWidth + 1);
  const firstLineHeight = () =>
    body
      .locator("code > span")
      .first()
      .evaluate((el) => el.getBoundingClientRect().height);
  await expect(body.locator(".markdown-code-token").first()).toBeAttached();
  await page.evaluate(() => document.fonts.ready);
  const wrappedHeight = await firstLineHeight();
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  await expect.poll(overflow).toBe(false);
  await page.evaluate(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async (text: string) => {
          Object.assign(window, { copiedCode: text });
        },
      },
    });
  });

  for (const wrapped of [false, true]) {
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-pressed", String(wrapped));
    await expect.poll(overflow).toBe(!wrapped);
    if (wrapped) {
      await expect.poll(firstLineHeight).toBeGreaterThan(wrappedHeight / 2);
    } else {
      await expect.poll(firstLineHeight).toBeLessThan(wrappedHeight / 2);
    }
    await expect(
      blocks.nth(1).getByRole("button", { name: "Wrap lines" }),
    ).toHaveAttribute("aria-pressed", "false");
    await page.evaluate(() => Object.assign(window, { copiedCode: undefined }));
    await blocks.first().locator(".markdown-code-copy").click();
    await expect
      .poll(() =>
        page.evaluate(
          () => (window as unknown as { copiedCode: string }).copiedCode,
        ),
      )
      .toBe(source);
  }
});

test("a nested wrap choice survives source growth and streaming completion", async ({
  page,
}) => {
  const nested = (intro: string, source: string) =>
    `- ${intro}\n\n  \`\`\`markdown\n  ${source.replaceAll("\n", "\n  ")}\n  \`\`\`\n`;
  await showExample(page, nested("Explanation", "first line"), true);
  const toggle = page.getByRole("button", { name: "Wrap lines" });
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  await toggle.click();
  const updated = nested(
    "A longer explanation before the fence",
    "first line\nsecond line",
  );
  await showExample(page, updated, true);
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await showExample(page, updated, false);
  await expect(page.locator(".word-fading")).toHaveCount(0);
  await expect(
    page.locator('[data-streamdown="code-block-body"] code'),
  ).toContainText("second line");
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
});
