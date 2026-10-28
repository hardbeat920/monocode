import { describe, expect, it } from "vitest";
import { isHiddenEditorHost, matchIndexAtSelection } from "./editorSearch";

describe("matchIndexAtSelection", () => {
  const matches = [
    { from: 2, to: 5 },
    { from: 8, to: 11 },
    { from: 14, to: 18 },
  ];

  it("returns the one-based match index for the current selection", () => {
    expect(matchIndexAtSelection(matches, { from: 8, to: 11 })).toBe(2);
    expect(matchIndexAtSelection(matches, { from: 14, to: 18 })).toBe(3);
  });

  it("returns zero when the selection is not a match", () => {
    expect(matchIndexAtSelection(matches, { from: 6, to: 8 })).toBe(0);
    expect(matchIndexAtSelection([], { from: 2, to: 5 })).toBe(0);
  });
});

describe("isHiddenEditorHost", () => {
  it("treats FilePane's hidden inactive tabs as unusable", () => {
    expect(
      isHiddenEditorHost({
        closest: (selector) =>
          selector.includes(".hidden") &&
          selector.includes("[aria-hidden='true']")
            ? {}
            : null,
      }),
    ).toBe(true);
  });

  it("treats the markdown source overlay as unusable", () => {
    expect(
      isHiddenEditorHost({
        closest: (selector) => (selector.includes(".invisible") ? {} : null),
      }),
    ).toBe(true);
  });

  it("keeps a visible editor", () => {
    expect(isHiddenEditorHost({ closest: () => null })).toBe(false);
  });
});
