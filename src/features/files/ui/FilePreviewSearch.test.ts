// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { handleEditorFindKey } from "../editor/editorSearch";
import { FilePreviewSearch, findPreviewTextMatches } from "./FilePreviewSearch";

const options = {
  caseSensitive: false,
  wholeWord: false,
  regexp: false,
};

describe("findPreviewTextMatches", () => {
  it("finds literal text case-insensitively", () => {
    expect(
      findPreviewTextMatches("Alpha beta ALPHA", "alpha", options).matches,
    ).toEqual([
      { from: 0, to: 5 },
      { from: 11, to: 16 },
    ]);
  });

  it("supports whole-word and regular-expression searches", () => {
    expect(
      findPreviewTextMatches("cat scatter cat", "cat", {
        ...options,
        wholeWord: true,
      }).matches,
    ).toEqual([
      { from: 0, to: 3 },
      { from: 12, to: 15 },
    ]);
    expect(
      findPreviewTextMatches("item-12 item-aa", "item-\\d+", {
        ...options,
        regexp: true,
      }).matches,
    ).toEqual([{ from: 0, to: 7 }]);
  });

  it("reports an invalid regular expression", () => {
    expect(
      findPreviewTextMatches("text", "[", { ...options, regexp: true }),
    ).toMatchObject({ matches: [], invalid: true });
  });
});

describe("FilePreviewSearch", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("opens from the find shortcut and navigates rendered matches", async () => {
    await act(async () =>
      root.render(
        createElement(
          FilePreviewSearch,
          { active: true, contentVersion: "Alpha beta alpha" },
          createElement(
            "div",
            { className: "markdown-preview", "aria-label": "Markdown preview" },
            createElement(
              "p",
              null,
              "Alpha ",
              createElement("strong", null, "beta"),
            ),
            createElement("p", null, "alpha"),
          ),
        ),
      ),
    );

    const shortcut = new KeyboardEvent("keydown", {
      key: "f",
      metaKey: true,
      cancelable: true,
    });
    await act(async () => {
      expect(handleEditorFindKey(shortcut)).toBe(true);
    });
    expect(shortcut.defaultPrevented).toBe(true);
    expect(
      container.querySelector("[data-file-preview-search-open]"),
    ).not.toBeNull();

    const input = container.querySelector<HTMLInputElement>(
      'input[aria-label="Find"]',
    )!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!.call(input, "Alpha beta");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(container.textContent).toContain("1 of 1");

    await act(async () => {
      Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!.call(input, "alpha");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(container.textContent).toContain("1 of 2");

    await act(async () => {
      input.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Enter",
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(container.textContent).toContain("2 of 2");

    await act(async () => {
      expect(
        handleEditorFindKey(
          new KeyboardEvent("keydown", { key: "Escape", cancelable: true }),
        ),
      ).toBe(true);
    });
    expect(container.querySelector('[role="search"]')).toBeNull();
    expect(
      container.querySelector("[data-file-preview-search-open]"),
    ).toBeNull();
  });
});
