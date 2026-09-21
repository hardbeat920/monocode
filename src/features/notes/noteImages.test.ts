import { describe, expect, it } from "vitest";
import {
  insertNoteImagesMarkdown,
  isNoteImagePath,
  noteImageMarkdown,
  type NoteImageAsset,
} from "./noteImages";

const image: NoteImageAsset = {
  name: "Architecture [draft].png",
  markdownPath: "/note-assets/note-1/123-architecture-draft.png",
};

describe("note image markdown", () => {
  it("escapes image names used as alt text", () => {
    expect(noteImageMarkdown(image)).toBe(
      "![Architecture \\[draft\\].png](/note-assets/note-1/123-architecture-draft.png)",
    );
  });

  it("inserts images as blocks at the cursor", () => {
    expect(insertNoteImagesMarkdown("BeforeAfter", 6, 6, [image])).toEqual({
      value:
        "Before\n\n![Architecture \\[draft\\].png](/note-assets/note-1/123-architecture-draft.png)\n\nAfter",
      cursor: 85,
    });
  });

  it("replaces the selection and separates multiple images", () => {
    const second = {
      name: "flow.png",
      markdownPath: "/note-assets/note-1/456-flow.png",
    };
    expect(
      insertNoteImagesMarkdown("Top\nreplace\nBottom", 4, 12, [image, second]),
    ).toEqual({
      value: [
        "Top",
        "",
        noteImageMarkdown(image),
        "",
        noteImageMarkdown(second),
        "",
        "Bottom",
      ].join("\n"),
      cursor: 129,
    });
  });

  it("recognizes only note asset references", () => {
    expect(isNoteImagePath(image.markdownPath)).toBe(true);
    expect(isNoteImagePath("https://example.com/image.png")).toBe(false);
  });
});
