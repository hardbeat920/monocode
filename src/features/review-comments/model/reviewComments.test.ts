import { beforeEach, describe, expect, it } from "vitest";
import {
  addReviewComment,
  clearReviewComments,
  formatReviewComments,
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
      "Please address these review comments:\n\n" +
        "1. `src/auth.ts`:4-5\n" +
        "   Return an explicit error.\n\n" +
        "   ```\n   if (!token) {\n     return;\n   }\n   ```\n\n" +
        "2. `src/session.ts` (file)\n" +
        "   Keep its public errors aligned with auth.",
    );
  });

  it("clears the in-memory draft", () => {
    addReviewComment({ path: "a.ts", startLine: 1, endLine: 1, snippet: "x", body: "note" });
    clearReviewComments();
    expect(reviewCommentsSnapshot()).toEqual([]);
  });
});
