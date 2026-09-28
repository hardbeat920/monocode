import { describe, expect, it } from "vitest";
import { diffCommentContext, diffCommentLocation } from "./diffComment";

describe("diff comments", () => {
  it("turns a comment on a current line into a context chip", () => {
    const target = {
      path: "src/auth.ts",
      line: {
        kind: "add" as const,
        text: "const token = readCookie();",
        oldNumber: null,
        newNumber: 42,
      },
    };

    expect(diffCommentLocation(target)).toBe("src/auth.ts:42");
    expect(diffCommentContext(target, " Please handle a missing cookie. ")).toEqual(
      {
        kind: "comment",
        path: "src/auth.ts",
        line: 42,
        change: "added",
        code: "const token = readCookie();",
        comment: "Please handle a missing cookie.",
      },
    );
  });

  it("uses the old line number for a removed line", () => {
    expect(
      diffCommentContext(
        {
          path: "src/old.ts",
          line: {
            kind: "del",
            text: "legacy();\r",
            oldNumber: 8,
            newNumber: null,
          },
        },
        "Keep this behavior.",
      ),
    ).toMatchObject({ line: 8, change: "removed", code: "legacy();" });
  });

  it("ignores an empty comment", () => {
    expect(
      diffCommentContext(
        {
          path: "README.md",
          line: {
            kind: "context",
            text: "Title",
            oldNumber: 1,
            newNumber: 1,
          },
        },
        " \n ",
      ),
    ).toBeNull();
  });
});
