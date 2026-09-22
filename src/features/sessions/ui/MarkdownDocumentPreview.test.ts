import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MarkdownDocumentPreview } from "./MarkdownDocumentPreview";

describe("MarkdownDocumentPreview", () => {
  it.each([
    "---\r\ntitle: Windows\r\n---\r\n\r\n# Heading",
    "\uFEFF---\ntitle: BOM\n---\n\n# Heading",
    "---\ntitle: End marker\n...\n\n# Heading",
    "---\n---\n\n# Heading",
  ])("separates leading frontmatter from the Markdown body: %j", (text) => {
    const html = renderToStaticMarkup(
      createElement(MarkdownDocumentPreview, {
        text,
        metadataLabel: "Properties",
      }),
    );

    expect(html).toContain("<details");
    expect(html).toContain("Properties");
    expect(html).toMatch(/<h1[^>]*>Heading<\/h1>/);
    expect(html).not.toContain('data-streamdown="horizontal-rule"');
  });

  it.each([
    "# No frontmatter",
    "---\ntitle: unfinished\n\n# Still ordinary Markdown",
    "Before\n\n---\ntitle: not at the start\n---",
  ])("leaves ordinary or unfinished Markdown untouched: %j", (text) => {
    const html = renderToStaticMarkup(
      createElement(MarkdownDocumentPreview, {
        text,
        metadataLabel: "Properties",
      }),
    );

    expect(html).not.toContain("<details");
  });

  it("shows frontmatter verbatim in the disclosure", () => {
    const html = renderToStaticMarkup(
      createElement(MarkdownDocumentPreview, {
        text: "---\ntitle: Example Note\ntags:\n  - alpha\n  - beta\n---\n\nBody.",
        metadataLabel: "Properties",
      }),
    );

    expect(html).toContain("title: Example Note\ntags:\n  - alpha\n  - beta");
    expect(html).toMatch(/<p[^>]*>Body\.<\/p>/);
    expect(html).not.toContain("<ul");
  });
});
