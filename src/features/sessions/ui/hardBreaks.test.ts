import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AgentMarkdown } from "./AgentMarkdown";

/**
 * https://github.com/hardbeat920/monocode/issues/591 - Markdown reads a single
 * newline as a soft break, which HTML collapses to a space, so consecutive
 * lines in a note ran together into one line of prose. A document is written
 * with hard-wrapped lines, so the preview shows each newline as a line break.
 */

function render(text: string, hardBreaks = true): string {
  return renderToStaticMarkup(
    createElement(AgentMarkdown, { text, hardBreaks }),
  );
}

describe("AgentMarkdown hard breaks", () => {
  it("keeps consecutive quote lines on their own lines", () => {
    expect(render("> first line\n> second line\n> third line")).toContain(
      "<p>first line<br/>second line<br/>third line</p>",
    );
  });

  it("keeps consecutive paragraph lines on their own lines", () => {
    expect(render("first line\nsecond line")).toContain(
      "<p>first line<br/>second line</p>",
    );
  });

  it("breaks inside emphasis and links, where Markdown kept the newline", () => {
    const markup = render(
      "**bold\ntext** and [link\ntext](https://example.com)",
    );
    expect(markup).toMatch(
      /<span[^>]*data-streamdown="strong"[^>]*>bold<br\/>text/,
    );
    expect(markup).toMatch(/>link<br\/>text</);
  });

  it("breaks between lines that are each a single inline element", () => {
    expect(render("*a*\n*b*")).toContain("<em>a</em><br/><em>b</em>");
    expect(render("`a`\n`b`")).toMatch(/<\/code><br\/><code/);
    expect(render("[a](https://x.com)\n[b](https://y.com)")).toMatch(
      /<\/a><br\/><a/,
    );
  });

  it("breaks the wrapped lines of a list item", () => {
    expect(render("- first item\n  continued")).toContain(
      ">first item<br/>continued</li>",
    );
  });

  it.each([
    ["a nested list", "- a\n  - b\n- c"],
    ["a loose list", "- a\n\n- b"],
    ["the paragraphs of a loose list item", "- a\n\n  second para\n- b"],
    [
      "a code block inside a list item",
      ["- a", "", "  ```txt", "  code line", "  ```", "- b"].join("\n"),
    ],
    [
      "an ordered list with a second paragraph",
      "1. first\n2. second\n\n   more of second",
    ],
  ])("adds no breaks around %s: %j", (_what, text) => {
    expect(render(text)).not.toContain("<br/>");
  });

  it.each(["two spaces  \nnext line", "backslash\\\nnext line"])(
    "leaves a Markdown hard break as the one break it already was: %j",
    (text) => {
      expect(render(text)).toContain("<br/>next line</p>");
      expect(render(text).match(/<br\/>/g)).toHaveLength(1);
    },
  );

  it("drops the space a soft break left behind", () => {
    expect(render("trailing space \nnext line")).toContain(
      "<p>trailing space<br/>next line</p>",
    );
  });

  it("does not end a paragraph with a break it has no line for", () => {
    expect(render("one line\n")).toContain("<p>one line</p>");
  });

  it("leaves code alone", () => {
    const markup = render(["```txt", "line one", "line two", "```"].join("\n"));
    expect(markup).not.toContain("<br/>");
    expect(markup).toContain("line two");
  });

  it("leaves a reply's prose alone, so it still reflows", () => {
    const markup = render("first line\nsecond line", false);
    expect(markup).toContain("first line\nsecond line");
    expect(markup).not.toContain("<br/>");
  });
});
