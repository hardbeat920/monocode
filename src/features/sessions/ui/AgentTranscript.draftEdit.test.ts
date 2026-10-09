// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AgentTranscript } from "./AgentTranscript";
import type { Block } from "../model/session";

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

const draft: Block = {
  id: "draft",
  role: "user",
  text: "Maybe later",
  draft: true,
};

function button(label: string) {
  return container.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!;
}

function press(key: string) {
  const editor = container.querySelector("textarea")!;
  act(() => {
    editor.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });
}

it("saves on Enter and returns focus to Send", () => {
  const onEditDraft = vi.fn(() => true);
  act(() =>
    root.render(
      createElement(AgentTranscript, { blocks: [draft], onEditDraft }),
    ),
  );

  act(() => button("Edit draft").click());
  press("Enter");

  expect(onEditDraft).toHaveBeenCalledWith(draft, "Maybe later");
  expect(container.querySelector("textarea")).toBeNull();
  expect(document.activeElement).toBe(button("Send draft"));
});

it("cancels on Escape and returns focus to Edit", () => {
  const onEditDraft = vi.fn(() => true);
  act(() =>
    root.render(
      createElement(AgentTranscript, { blocks: [draft], onEditDraft }),
    ),
  );

  act(() => button("Edit draft").click());
  press("Escape");

  expect(onEditDraft).not.toHaveBeenCalled();
  expect(container.querySelector("textarea")).toBeNull();
  expect(document.activeElement).toBe(button("Edit draft"));
});
