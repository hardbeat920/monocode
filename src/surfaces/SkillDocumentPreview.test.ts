import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SkillDocumentPreview } from "./SkillDocumentPreview";

describe("skill document metadata", () => {
  it.each([
    "---\r\nname: windows\r\n---\r\n\r\n# Instructions",
    "\uFEFF---\nname: windows\n---\n\n# Instructions",
    "---\nname: windows\n...\n\n# Instructions",
    "---\n---\n\n# Instructions",
  ])(
    "recognizes frontmatter delimiters without losing the body: %j",
    (text) => {
      const html = renderToStaticMarkup(
        createElement(SkillDocumentPreview, { text }),
      );
      expect(html).toContain("<details");
      expect(html).toMatch(/<h1[^>]*>Instructions<\/h1>/);
      expect(html).not.toContain("<h2");
    },
  );
});
