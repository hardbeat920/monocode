import { beforeEach, describe, expect, it } from "vitest";
import {
  addReviewComment,
  clearReviewComments,
  formatReviewComments,
  remapReviewCommentLines,
  reviewCommentsSnapshot,
} from "./reviewComments";

describe("review comments", () => {
  beforeEach(clearReviewComments);

  it("formats code and file comments as one composer message", () => {
    addReviewComment({
      path: "src/auth.ts",
      startLine: 4,
      endLine: 5,
      snippet: "if (!token) {\n  return;\n}",
      body: "Return an explicit error.",
    });
    addReviewComment({
      path: "src/session.ts",
      startLine: 0,
      endLine: 0,
      snippet: "",
      body: "Keep its public errors aligned with auth.",
    });

    expect(formatReviewComments()).toBe(
      "@src/auth.ts (lines 4-5)\n" +
        "Return an explicit error.\n\n" +
        "@src/session.ts (file)\n" +
        "Keep its public errors aligned with auth.",
    );
  });

  it("clears the in-memory draft", () => {
    addReviewComment({ path: "a.ts", startLine: 1, endLine: 1, snippet: "x", body: "note" });
    clearReviewComments();
    expect(reviewCommentsSnapshot()).toEqual([]);
  });

  it("can clear only a submitted comment snapshot", () => {
    const first = addReviewComment({
      path: "a.ts",
      startLine: 1,
      endLine: 1,
      snippet: "x",
      body: "first",
    });
    const second = addReviewComment({
      path: "a.ts",
      startLine: 2,
      endLine: 2,
      snippet: "y",
      body: "second",
    });
    clearReviewComments(new Set([first.id]));
    expect(reviewCommentsSnapshot()).toEqual([second]);
  });

  it("preserves deleted-line context and follows edits to its file", () => {
    addReviewComment({
      path: "a.ts",
      startLine: 2,
      endLine: 3,
      snippet: "x",
      body: "note",
      deleted: true,
    });
    remapReviewCommentLines("a.ts", (line) => line + 2);
    expect(formatReviewComments()).toBe(
      "@a.ts (lines 4-5) (deleted)\nnote",
    );
  });
});
