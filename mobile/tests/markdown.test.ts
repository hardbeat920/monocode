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
