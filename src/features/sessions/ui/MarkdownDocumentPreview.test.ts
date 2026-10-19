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

  // https://github.com/hardbeat920/monocode/issues/591
  it("keeps consecutive lines on their own lines", () => {
    const html = renderToStaticMarkup(
      createElement(MarkdownDocumentPreview, {
        text: "> first line\n> second line\n> third line",
        metadataLabel: "Properties",
      }),
    );

    expect(html).toContain("<p>first line<br/>second line<br/>third line</p>");
  });
});

describe("MarkdownDocumentPreview images", () => {
  const render = (text: string, documentPath?: string) =>
    renderToStaticMarkup(
      createElement(MarkdownDocumentPreview, {
        text,
        metadataLabel: "Properties",
        cwd: "/repo",
        documentPath,
      }),
    );

  it("shows a centered raw HTML image from the document's project", () => {
    const html = render(
      '<p align="center">\n  <img src=".github/readme/hero.png" width="600" alt="Hero">\n</p>',
      "/repo/README.md",
    );

    expect(html).toMatch(/<p align="center">/);
    expect(html).toContain(
      'data-document-image="/repo/.github/readme/hero.png"',
    );
    expect(html).toContain('aria-label="Hero"');
    expect(html).toContain("width:600px");
  });

  it("shows Markdown images in a table, relative to the document's folder", () => {
    const html = render(
      "| Chat | Files |\n| --- | --- |\n| ![Chat](../.github/readme/chat.png) | ![Files](shots/files.jpg) |",
      "/repo/docs/guide.md",
    );

    expect(html).toContain("<table");
    expect(html).toContain(
      'data-document-image="/repo/.github/readme/chat.png"',
    );
    expect(html).toContain('data-document-image="/repo/docs/shots/files.jpg"');
  });

  it("shows a picture's image and drops its sources", () => {
    const html = render(
      '<picture>\n  <source media="(prefers-color-scheme: dark)" srcset="logo-dark.svg">\n  <img src="logo.svg" alt="Logo" width="88">\n</picture>',
      "/repo/README.md",
    );

    expect(html).toContain('data-document-image="/repo/logo.svg"');
    expect(html).not.toContain("<source");
    expect(html).not.toContain("logo-dark.svg");
  });

  it("shows alt text for images outside the project and remote images", () => {
    const html = render(
      [
        '<img src="../../etc/secret.png" alt="Escaped">',
        "",
        "![build](https://img.shields.io/badge/build-passing-green.svg)",
        "",
        '<img src="file:///etc/x.png" alt="Local file">',
      ].join("\n"),
      "/repo/README.md",
    );

    expect(html).not.toContain("data-document-image");
    expect(html).not.toContain("<img");
    expect(html).toMatch(/data-image-alt=""[^>]*>Escaped<\/span>/);
    expect(html).toMatch(/data-image-alt=""[^>]*>build<\/span>/);
    expect(html).toMatch(/data-image-alt=""[^>]*>Local file<\/span>/);
  });

  it("leaves images alone when no document is shown", () => {
    const html = render('<img src="hero.png" alt="Hero">\n\n![Shot](shot.png)');

    expect(html).not.toContain("data-document-image");
    expect(html).not.toContain("data-image-alt");
  });
});
