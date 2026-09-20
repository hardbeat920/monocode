import { describe, expect, it } from "vitest";
import {
  buildThreadTitlePrompt,
  parseGeneratedSessionTitle,
} from "./sessionTitle";

describe("session title metadata", () => {
  it("asks the title pass for one optional work item", () => {
    expect(buildThreadTitlePrompt("Fix PR #42")).toContain(
      "title and workItem",
    );
  });

  it("accepts a referenced PR number", () => {
    expect(
      parseGeneratedSessionTitle(
        '{"title":"Fix session links","workItem":{"kind":"pr","number":42}}',
        "Please fix PR #42",
      ),
    ).toEqual({
      title: "Fix session links",
      workItem: { kind: "pr", number: 42 },
    });
  });

  it("drops a model-invented number without losing the title", () => {
    expect(
      parseGeneratedSessionTitle(
        '{"title":"Fix session links","workItem":{"kind":"issue","number":99}}',
        "Please fix the session links",
      ),
    ).toEqual({ title: "Fix session links", workItem: null });
  });

  it("keeps compatibility with a bare generated title", () => {
    expect(parseGeneratedSessionTitle("Fix session links", "anything")).toEqual(
      { title: "Fix session links", workItem: null },
    );
  });
});
