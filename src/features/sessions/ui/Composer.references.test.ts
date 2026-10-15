// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Composer } from "./Composer";
import { newChatReference } from "../model/chatReferences";

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));

let container: HTMLDivElement;
let root: Root;
const submit = vi.fn();
const change = vi.fn();
const refs = [newChatReference("Cited line", "why this?")];

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  submit.mockReset();
  change.mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function render(initialDraft: string, references = refs) {
  await act(async () =>
    root.render(
      createElement(Composer, {
        focused: false,
        harness: "codex",
        model: "",
        runtimeMode: "supervised",
        executionCwd: "~",
        initialDraft,
        hideTopBar: true,
        references,
        onReferencesChange: change,
        onFocus: vi.fn(),
        onCwdChange: vi.fn(),
        onModelChange: vi.fn(),
        onRuntimeModeChange: vi.fn(),
        onSubmit: submit,
      } as never),
    ),
  );
}
const send = () =>
  act(async () =>
    container.querySelector<HTMLButtonElement>('[aria-label="Send"]')!.click(),
  );

it("shows one pill and leaves the typed draft alone", async () => {
  await render("Draft");
  expect(container.textContent).toContain("1 annotation");
  expect(container.querySelector("textarea")!.value).toBe("Draft");
});

it("sends the cited passages after the message, then clears them", async () => {
  await render("Draft");
  await send();
  expect(submit.mock.calls[0][0]).toBe(
    "Draft\n\nReference 1:\n> Cited line\n\nNote: why this?",
  );
  expect(change).toHaveBeenCalledWith([]);
});

it("can send the cited passages with no typed text", async () => {
  await render("");
  await send();
  expect(submit.mock.calls[0][0]).toBe(
    "Reference 1:\n> Cited line\n\nNote: why this?",
  );
});

it("keeps the cited passages when the app declines the message", async () => {
  submit.mockReturnValue(false);
  await render("Draft");
  await send();
  expect(change).not.toHaveBeenCalled();
});

it("puts the cursor in the input when a reference is added", async () => {
  await render("Draft", []);
  expect(document.activeElement).not.toBe(container.querySelector("textarea"));
  await render("Draft", refs);
  await act(async () => {
    await new Promise((resolve) => requestAnimationFrame(resolve));
  });
  expect(document.activeElement).toBe(container.querySelector("textarea"));
});
