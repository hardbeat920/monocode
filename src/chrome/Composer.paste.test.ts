// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { Composer } from "./Composer";
import { copyMessage } from "../lib/clipboard";

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
