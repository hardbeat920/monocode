// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Composer } from "./Composer";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async () => () => undefined,
  }),
}));

const PNG_BYTES = new Uint8Array([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
]);

let container: HTMLDivElement;
let root: Root;
let submit: ReturnType<typeof vi.fn>;

function render(text = "") {
  act(() => {
    root.render(
      createElement(Composer, {
        focused: false,
        harness: "codex",
        model: "",
        runtimeMode: "supervised",
        executionCwd: "/repo",
        initialDraft: text,
        hideTopBar: true,
        onFocus: vi.fn(),
        onCwdChange: vi.fn(),
        onModelChange: vi.fn(),
        onRuntimeModeChange: vi.fn(),
        onSubmit: submit,
      }),
    );
  });
  return container.querySelector("textarea")!;
}

function paste(target: HTMLTextAreaElement, text = "") {
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", {
    value: { getData: () => text, files: [], items: [] },
  });
  act(() => {
    target.dispatchEvent(event);
  });
  return event;
}

async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
}

async function send() {
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('button[aria-label="Send"]')!
      .click(),
  );
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  submit = vi.fn();
  invoke.mockReset();
  invoke.mockImplementation(async (command: string) => {
    if (command === "clipboard_image") return PNG_BYTES.buffer;
    return [];
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("attaches an image the paste event reports as neither file nor text", async () => {
  const textarea = render();
  const event = paste(textarea);
  await settle();
  await send();

  expect(event.defaultPrevented).toBe(true);
  expect(invoke).toHaveBeenCalledWith("clipboard_image");
  expect(submit.mock.calls[0][1]).toEqual([
    expect.objectContaining({
      name: "clipboard-image.png",
      mimeType: "image/png",
      kind: "image",
    }),
  ]);
});

it("leaves a text paste to the webview instead of reading the native clipboard", async () => {
  const textarea = render();
  const event = paste(textarea, "pasted words");
  await settle();

  expect(event.defaultPrevented).toBe(false);
  expect(invoke).not.toHaveBeenCalled();
  expect(submit).not.toHaveBeenCalled();
});

it("reports why a clipboard image could not be attached", async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === "clipboard_image")
      throw "Clipboard image is too large to attach (maximum 20 MB).";
    return [];
  });
  const textarea = render();
  paste(textarea);
  await settle();

  expect(container.querySelector('[role="alert"]')?.textContent).toBe(
    "Clipboard image is too large to attach (maximum 20 MB).",
  );
  expect(
    container.querySelector('[aria-label^="Open clipboard-image"]'),
  ).toBeNull();
});
