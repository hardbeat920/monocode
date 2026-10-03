import { describe, expect, it } from "vitest";
import {
  insertSavedPrompt,
  promptTitleFrom,
  promptTokenAt,
  rankSavedPrompts,
  savedPromptError,
  savedPromptLabel,
  type SavedPrompt,
} from "./savedPrompts";

function prompt(
  id: string,
  title: string,
  body = `${title} body`,
): SavedPrompt {
  return {
    id,
    title,
    body,
    pinned: false,
    useCount: 0,
    createdAt: 1,
    updatedAt: 1,
  };
}

describe("promptTokenAt", () => {
  it("opens on ! at the start or after whitespace and reads the query up to the caret", () => {
    expect(promptTokenAt("!", 1)).toEqual({ start: 0, end: 1, query: "" });
    expect(promptTokenAt("please !rev", 11)).toEqual({
      start: 7,
      end: 11,
      query: "rev",
    });
    expect(promptTokenAt("line\n!fix it", 9)).toEqual({
      start: 5,
      end: 9,
      query: "fix",
    });
  });

  it("ignores ! inside words, operators, and a caret before the !", () => {
    expect(promptTokenAt("Done!", 5)).toBeNull();
    expect(promptTokenAt("a!=b", 3)).toBeNull();
    expect(promptTokenAt("!!", 2)).toBeNull();
    expect(promptTokenAt("say !hi", 4)).toBeNull();
    expect(promptTokenAt("!rev now", 8)).toBeNull();
  });
});

describe("insertSavedPrompt", () => {
  it("replaces the whole token, keeps the rest, and puts the caret after the text", () => {
    const text = "Please !re and then";
    const token = promptTokenAt(text, 9)!;
    expect(insertSavedPrompt(text, token, "Review this PR")).toEqual({
      text: "Please Review this PR and then",
      cursor: "Please Review this PR".length,
    });
  });

  it("adds a space before text that would otherwise touch the inserted prompt", () => {
    const token = { start: 0, end: 4, query: "rev" };
    expect(insertSavedPrompt("!rev", token, "Body").text).toBe("Body");
    expect(insertSavedPrompt("!revX", { ...token, end: 4 }, "Body").text).toBe(
      "Body X",
    );
  });
});

describe("rankSavedPrompts", () => {
  const prompts = [
    prompt("pinned", "Explain like I am new"),
    prompt("review", "Review this PR"),
    prompt("body", "Untitled", "Please review the tests"),
    prompt("fix", "Fix lint"),
  ];

  it("keeps the stored order (pinned, most used) without a query", () => {
    expect(rankSavedPrompts(prompts, "").map((p) => p.id)).toEqual([
      "pinned",
      "review",
      "body",
      "fix",
    ]);
    expect(rankSavedPrompts(prompts, "", 2)).toHaveLength(2);
  });

  it("ranks title prefix, then title text, then body matches", () => {
    expect(rankSavedPrompts(prompts, "rev").map((p) => p.id)).toEqual([
      "review",
      "body",
    ]);
    expect(rankSavedPrompts(prompts, "zzz")).toEqual([]);
  });
});

describe("titles and validation", () => {
  it("derives a short title from the first non-empty line", () => {
    expect(promptTitleFrom("\n  # Review the diff\nMore")).toBe(
      "Review the diff",
    );
    expect(promptTitleFrom("word ".repeat(30), 20)).toBe(
      "word word word word…",
    );
    expect(savedPromptLabel({ title: " ", body: "Fix it\nnow" })).toBe(
      "Fix it",
    );
  });

  it("requires text and limits sizes", () => {
    expect(savedPromptError("", "  ")).toBe("Add the text to insert.");
    expect(savedPromptError("t".repeat(201), "ok")).toBe(
      "The title is too long.",
    );
    expect(savedPromptError("", "ok")).toBeNull();
  });
});
