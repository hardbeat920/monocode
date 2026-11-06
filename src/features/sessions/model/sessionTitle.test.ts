import { describe, expect, it } from "vitest";
import {
  buildThreadTitlePrompt,
  parseGeneratedSessionTitle,
  shouldGenerateSessionTitle,
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

  it("drops an issue number that only appears inside a ticket key", () => {
    expect(
      parseGeneratedSessionTitle(
        '{"title":"Implement SW-29","workItem":{"kind":"issue","number":29}}',
        "/dev-implement SW-29",
      ),
    ).toEqual({ title: "Implement SW-29", workItem: null });
    expect(buildThreadTitlePrompt("SW-29")).toContain("ticket key");
  });

  it("still accepts a number that also stands alone or follows a plain hyphenated word", () => {
    expect(
      parseGeneratedSessionTitle(
        '{"title":"Fix issue 29","workItem":{"kind":"issue","number":29}}',
        "SW-29 is about issue 29",
      )?.workItem,
    ).toEqual({ kind: "issue", number: 29 });
    expect(
      parseGeneratedSessionTitle(
        '{"title":"Fix issue 29","workItem":{"kind":"issue","number":29}}',
        "Fix issue-29",
      )?.workItem,
    ).toEqual({ kind: "issue", number: 29 });
  });

  it("keeps compatibility with a bare generated title", () => {
    expect(parseGeneratedSessionTitle("Fix session links", "anything")).toEqual(
      { title: "Fix session links", workItem: null },
    );
  });

  it("can refresh a generated title for an event added to an existing session", () => {
    expect(shouldGenerateSessionTitle(false, false, true)).toBe(true);
    expect(shouldGenerateSessionTitle(false, false)).toBe(false);
    expect(shouldGenerateSessionTitle(true, true)).toBe(true);
  });
});
