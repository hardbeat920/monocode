// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TranscriptSelectionMenu } from "./TranscriptSelectionMenu";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("TranscriptSelectionMenu", () => {
  it("offers the selected text to both chat and notes", () => {
    const onAddToChat = vi.fn();
    const onAddToNotes = vi.fn();
    const onDismiss = vi.fn();
    act(() =>
      root.render(
        createElement(TranscriptSelectionMenu, {
          selection: {
            text: "A useful link",
            rect: new DOMRect(10, 20, 100, 20),
          },
          onAddToChat,
          onAddToNotes,
          onDismiss,
        }),
      ),
    );

    const toolbar = document.querySelector(
      '[role="toolbar"][aria-label="Selected text actions"]',
    );
    expect(toolbar?.textContent).toContain("Add to chat");
    expect(toolbar?.textContent).toContain("Add to notes");

    const notes = Array.from(toolbar?.querySelectorAll("button") ?? []).find(
      (button) => button.textContent?.includes("Add to notes"),
    );
    act(() => notes?.click());

    expect(onAddToNotes).toHaveBeenCalledWith("A useful link");
    expect(onAddToChat).not.toHaveBeenCalled();
    expect(onDismiss).toHaveBeenCalledOnce();
  });
});
