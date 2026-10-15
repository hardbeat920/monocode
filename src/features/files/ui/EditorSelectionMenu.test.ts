// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { EditorSelectionMenu } from "./EditorSelectionMenu";
import {
  ADD_TO_CHAT_EVENT,
  type AddToChatRequest,
} from "../../sessions/model/quoteDraft";

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

const selection = {
  path: "src/app.ts",
  startLine: 3,
  endLine: 5,
  text: "const x = 1;",
  from: 20,
  to: 32,
  anchor: new DOMRect(10, 20, 100, 20),
};

it("asks for a note, then cites the passage with its file and lines", () => {
  const requests: AddToChatRequest[] = [];
  const listener = (event: Event) =>
    requests.push((event as CustomEvent<AddToChatRequest>).detail);
  window.addEventListener(ADD_TO_CHAT_EVENT, listener);
  try {
    const show = (current: typeof selection | null) =>
      act(() =>
        root.render(
          createElement(EditorSelectionMenu, {
            selection: current,
            onDismiss: vi.fn(),
          }),
        ),
      );
    show(selection);
    act(() =>
      document
        .querySelector<HTMLButtonElement>('[role="toolbar"] button')!
        .click(),
    );
    expect(requests).toHaveLength(0);

    show(null);
    const field = document.querySelector<HTMLTextAreaElement>(
      '[role="dialog"] textarea',
    )!;
    const set = Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!;
    act(() => {
      set.call(field, "why is this a const?");
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    act(() => {
      field.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });

    expect(requests).toHaveLength(1);
    expect(requests[0].reference).toEqual({
      text: "const x = 1;",
      note: "why is this a const?",
      source: {
        label: "@src/app.ts (lines 3-5)",
        path: "src/app.ts",
        from: 20,
        to: 32,
      },
    });
    // A chat with no pane yet is seeded from the plain text.
    expect(requests[0].text).toContain("const x = 1;");
    expect(requests[0].text).toContain("Note: why is this a const?");
  } finally {
    window.removeEventListener(ADD_TO_CHAT_EVENT, listener);
  }
});
