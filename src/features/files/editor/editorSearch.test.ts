import { describe, expect, it } from "vitest";
import { isHiddenEditorHost } from "./editorSearch";

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
