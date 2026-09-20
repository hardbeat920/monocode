import { describe, expect, it } from "vitest";
import { diffCommentLocation, formatDiffComment } from "./diffComment";

describe("diff comments", () => {
  it("formats a comment on a current line for the composer", () => {
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
    expect(formatDiffComment(target, " Please handle a missing cookie. ")).toBe(
      [
        "Diff comment on `src/auth.ts:42`:",
        "",
        "> +const token = readCookie();",
        "",
        "Please handle a missing cookie.",
      ].join("\n"),
    );
  });

  it("uses the old line number and labels deleted lines", () => {
    expect(
      formatDiffComment(
        {
          path: "src/old.ts",
          line: {
            kind: "del",
            text: "legacy();",
            oldNumber: 8,
            newNumber: null,
          },
        },
        "Keep this behavior.",
      ),
    ).toContain("`src/old.ts:8` (deleted line)");
  });

  it("ignores an empty comment", () => {
    expect(
      formatDiffComment(
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
    ).toBe("");
  });
});
