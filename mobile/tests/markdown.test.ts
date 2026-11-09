import { describe, expect, it } from "vitest";

import { parseInline, parseMarkdown } from "@/features/sessions/markdownParser";
import {
  formatWorkingDuration,
  hasTurnMetrics,
  turnMetricsSheet,
} from "@/features/sessions/transcriptFormat";

describe("reply markdown", () => {
  it("reads the blocks agents write", () => {
    const blocks = parseMarkdown(
      [
        "## Summary",
        "",
        "Fixed the **bug** in `app.ts`.",
        "",
        "- first",
        "- second",
        "",
        "1. one",
        "2. two",
        "",
        "```ts",
        "const a = 1;",
        "```",
        "",
        "> quoted",
        "",
        "| a | b |",
        "| - | - |",
        "| 1 | 2 |",
      ].join("\n"),
    );
    expect(blocks.map((block) => block.type)).toEqual([
      "heading",
      "paragraph",
      "list",
      "list",
      "code",
      "quote",
      "table",
    ]);
    expect(blocks[4]).toEqual({ type: "code", lang: "ts", text: "const a = 1;" });
    expect(blocks[3]).toMatchObject({ ordered: true, start: 1 });
    expect(blocks[6]).toMatchObject({ rows: { length: 2 } });
  });

  it("keeps an unclosed fence open while the reply streams", () => {
    expect(parseMarkdown("```\nlet x")).toEqual([{ type: "code", lang: "", text: "let x" }]);
  });

  it("reads emphasis without breaking snake_case or stray stars", () => {
    expect(parseInline("a **b** and *c* in snake_case_name")).toEqual([
      { type: "text", text: "a " },
      { type: "strong", children: [{ type: "text", text: "b" }] },
      { type: "text", text: " and " },
      { type: "em", children: [{ type: "text", text: "c" }] },
      { type: "text", text: " in snake_case_name" },
    ]);
    expect(parseInline("2 * 3 * 4")).toEqual([{ type: "text", text: "2 * 3 * 4" }]);
  });

  it("reads links, bare URLs and inline code", () => {
    expect(parseInline("see [docs](https://a.dev) or https://b.dev.")).toEqual([
      { type: "text", text: "see " },
      { type: "link", href: "https://a.dev", children: [{ type: "text", text: "docs" }] },
      { type: "text", text: " or " },
      { type: "link", href: "https://b.dev", children: [{ type: "text", text: "https://b.dev" }] },
      { type: "text", text: "." },
    ]);
    expect(parseInline("`a*b*c`")).toEqual([{ type: "code", text: "a*b*c" }]);
  });
});

describe("turn labels", () => {
  it("matches the desktop's fold line", () => {
    expect(formatWorkingDuration(64_000, "Claude Opus", true)).toBe(
      "Claude Opus worked for 1m 4s",
    );
    expect(formatWorkingDuration(null)).toBe("Working…");
    expect(formatWorkingDuration(5_000, "Sonnet")).toBe("Sonnet working for 5s");
  });
});

describe("turn metrics", () => {
  it("splits into rates and full token counts", () => {
    expect(
      turnMetricsSheet(
        { cacheHitPercent: 91.6, inputTokens: 12_345, outputTokens: 2_400, cacheReadTokens: 1_200_000 },
        60_000,
      ),
    ).toEqual({
      highlights: [
        { label: "Cache hit", value: "92", unit: "%" },
        { label: "Output speed", value: "40", unit: "tok/s" },
        { label: "Duration", value: "1m" },
      ],
      tokens: [
        { label: "Input", value: "12,345" },
        { label: "Output", value: "2,400" },
        { label: "Cache read", value: "1,200,000" },
      ],
    });
    expect(hasTurnMetrics({ inputTokens: 0 })).toBe(false);
  });
});

describe("provider text", () => {
  const html = (text: string) => parseMarkdown(text, { html: true });

  it("reads the pull request a phone showed as raw tags", () => {
    const blocks = html(
      [
        "### Before",
        '<img width="699" height="1017" alt="IMG_0189" src="https://github.com/user-attachments/assets/98c9" />',
        "",
        "### Checklist",
        "- [x] I ran `npm run check`",
        "- [ ] Not this one",
        "",
        "<!-- This is an auto-generated comment: release notes by coderabbit.ai -->",
        "<!-- coderabbit-ai-generated:v1 -->",
        "",
        "## Summary by CodeRabbit",
      ].join("\r\n"),
    );
    expect(blocks.map((block) => block.type)).toEqual([
      "heading",
      "image",
      "heading",
      "list",
      "heading",
    ]);
    expect(blocks[1]).toMatchObject({
      image: {
        src: "https://github.com/user-attachments/assets/98c9",
        alt: "IMG_0189",
        width: 699,
        height: 1017,
      },
    });
    expect(blocks[3]).toMatchObject({ tasks: [true, false] });
    expect(blocks[3]).toMatchObject({
      items: [
        [{ inline: [{ text: "I ran " }, { type: "code", text: "npm run check" }] }],
        [{ inline: [{ text: "Not this one" }] }],
      ],
    });
  });

  it("drops comments across lines but not in code", () => {
    const blocks = html(
      [
        "before <!-- gone --> after `<!-- kept -->`",
        "",
        "<!-- a template hint",
        "over ![two](https://x.test/a.png) lines -->",
        "```html",
        "<!-- also kept -->",
        "```",
      ].join("\n"),
    );
    expect(blocks).toMatchObject([
      { type: "paragraph", inline: [{ text: "before  after " }, { text: "<!-- kept -->" }] },
      { type: "code", text: "<!-- also kept -->" },
    ]);
  });

  it("stands an image apart from the words around it", () => {
    expect(html("Before ![a](https://x.test/a.png) after")).toMatchObject([
      { type: "paragraph", inline: [{ text: "Before" }] },
      { type: "image", image: { src: "https://x.test/a.png", alt: "a" } },
      { type: "paragraph", inline: [{ text: "after" }] },
    ]);
  });

  it("keeps the link around an image", () => {
    const image = { type: "image", image: { src: "https://x.test/a.png" } };
    expect(html("[![a](https://x.test/a.png)](https://x.test/page)")).toMatchObject([
      { ...image, href: "https://x.test/page" },
    ]);
    expect(html('<a href="https://x.test/page"><img src="https://x.test/a.png"></a>')).toMatchObject([
      { ...image, href: "https://x.test/page" },
    ]);
  });

  it("finds images in layout tags and leaves no empty text", () => {
    expect(
      html(
        [
          '<p align="center">',
          "  <picture>",
          '    <source srcset="https://x.test/dark.png">',
          '    <img src="https://x.test/a.png" width="50%">',
          "  </picture>",
          "</p>",
        ].join("\n"),
      ),
    ).toEqual([{ type: "image", image: { type: "image", src: "https://x.test/a.png", alt: "" } }]);
  });

  it("keeps images in table cells for the table to draw", () => {
    const [table] = html(
      ["| Before | After |", "|---|---|", '| <img src="https://x.test/a.png"> | text |'].join("\n"),
    );
    expect(table).toMatchObject({
      type: "table",
      rows: [
        [[{ text: "Before" }], [{ text: "After" }]],
        [[{ type: "image", src: "https://x.test/a.png" }], [{ text: "text" }]],
      ],
    });
  });

  it("folds details, nested ones whole", () => {
    expect(
      html(
        [
          "<details><summary><b>Walkthrough</b></summary>",
          "",
          "Outer **text**",
          "",
          "<details open>",
          "<summary>Inner</summary>",
          "",
          "Deep",
          "</details>",
          "</details>",
          "",
          "After",
        ].join("\n"),
      ),
    ).toMatchObject([
      {
        type: "details",
        open: false,
        summary: [{ type: "strong", children: [{ text: "Walkthrough" }] }],
        children: [
          { type: "paragraph" },
          {
            type: "details",
            open: true,
            summary: [{ text: "Inner" }],
            children: [{ type: "paragraph", inline: [{ text: "Deep" }] }],
          },
        ],
      },
      { type: "paragraph", inline: [{ text: "After" }] },
    ]);
  });

  it("shows an unclosed details rather than losing it", () => {
    expect(html("<details>\n<summary>Open</summary>\n\nStill here")).toMatchObject([
      { type: "details", children: [{ inline: [{ text: "Still here" }] }] },
    ]);
  });

  it("reads inline tags and leaves ones that are not HTML", () => {
    expect(parseInline("<b>bold</b> <code>x</code> <a href='https://x.test'>go</a>", true)).toMatchObject([
      { type: "strong" },
      { text: " " },
      { type: "code", text: "x" },
      { text: " " },
      { type: "link", href: "https://x.test" },
    ]);
    expect(parseInline("line<br>next<br/>", true).map((node) => "text" in node && node.text).join("")).toBe(
      "line\nnext\n",
    );
    expect(parseInline("a Vec<T> and Option<String>", true)).toEqual([
      { type: "text", text: "a Vec<T> and Option<String>" },
    ]);
  });

  it("leaves an agent's words as written", () => {
    const text = 'Use <img src="a.png"> or ![a](b.png), not <!-- this -->';
    expect(parseMarkdown(text)).toEqual([{ type: "paragraph", inline: parseInline(text) }]);
    expect(parseInline(text).some((node) => node.type === "image")).toBe(false);
    expect(parseMarkdown("<details>\n<summary>Hi</summary>\n</details>")[0].type).toBe("paragraph");
  });

  it("gives task boxes to any Markdown, and only to lists that have them", () => {
    expect(parseMarkdown("- [x] done\n- plain")[0]).toMatchObject({ tasks: [true, undefined] });
    expect(parseMarkdown("- plain")[0]).not.toHaveProperty("tasks");
  });
});
