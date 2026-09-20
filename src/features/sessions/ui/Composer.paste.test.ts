// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { Composer } from "./Composer";
import { copyMessage } from "../../../platform/tauri/clipboard";

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));

it("pastes copied message text at the selection together with its attachment", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const submit = vi.fn();
  let written: ClipboardItem[] = [];
  vi.spyOn(navigator.clipboard, "write").mockImplementation(async (items) => {
    written = items;
  });
  try {
    await copyMessage("See image", [
      {
        id: "a",
        name: "shot.png",
        mimeType: "image/png",
        kind: "image",
        size: 3,
        data: "YWJj",
      },
    ]);
    const html = await (await written[0].getType("text/html")).text();
    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: false,
          harness: "codex",
          model: "",
          runtimeMode: "supervised",
          executionCwd: "~",
          initialDraft: "Before replace after",
          hideTopBar: true,
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit: submit,
        }),
      ),
    );
    const textarea = container.querySelector("textarea")!;
    textarea.setSelectionRange(7, 14);
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", {
      value: {
        getData: (type: string) => (type === "text/html" ? html : "See image"),
        files: [],
        items: [],
      },
    });
    await act(async () => {
      textarea.dispatchEvent(event);
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    expect(textarea.value).toBe("Before See image after");
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[aria-label="Send"]')!
        .click(),
    );
    expect(submit.mock.calls[0][0]).toBe("Before See image after");
    expect(submit.mock.calls[0][1][0]).toMatchObject({
      name: "shot.png",
      data: "YWJj",
    });
  } finally {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});

it("leaves a pasted GitHub pull request URL as editable text", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const submit = vi.fn();
  const onDraftChange = vi.fn();
  const url = "https://github.com/hardbeat920/monocode/pull/318";
  const props = {
    focused: false,
    harness: "codex" as const,
    model: "",
    runtimeMode: "supervised" as const,
    executionCwd: "/repo",
    initialDraft: "What is this  explain in simple terms",
    hideTopBar: true,
    onFocus: vi.fn(),
    onCwdChange: vi.fn(),
    onModelChange: vi.fn(),
    onRuntimeModeChange: vi.fn(),
    onSubmit: submit,
    onDraftChange,
  };
  try {
    await act(async () => root.render(createElement(Composer, props)));
    const textarea = container.querySelector("textarea")!;
    const insertionPoint = "What is this ".length;
    textarea.setSelectionRange(insertionPoint, insertionPoint);
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", {
      value: {
        getData: (type: string) => (type === "text/plain" ? url : ""),
        files: [],
        items: [],
      },
    });

    await act(async () => textarea.dispatchEvent(event));

    expect(event.defaultPrevented).toBe(false);
    expect(container.querySelector("[data-composer-link-chip]")).toBeNull();

    await act(async () => {
      textarea.setRangeText(url, insertionPoint, insertionPoint, "end");
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const expected = `What is this ${url} explain in simple terms`;
    expect(textarea.value).toBe(expected);
    expect(onDraftChange).toHaveBeenLastCalledWith(expected);

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[aria-label="Send"]')!
        .click(),
    );
    expect(submit).toHaveBeenCalledWith(expected, [], { intent: "default" });
  } finally {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});
