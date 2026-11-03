// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { MonoComposer } from "./MonoComposer";

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));

function pressEnter(field: HTMLTextAreaElement, keyCode: number) {
  const event = new KeyboardEvent("keydown", {
    key: "Enter",
    bubbles: true,
    cancelable: true,
  });
  Object.defineProperty(event, "keyCode", { value: keyCode });
  act(() => {
    field.dispatchEvent(event);
  });
}

it("doesn't send on the Enter that picks an IME candidate", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const onSubmit = vi.fn();
  try {
    await act(async () =>
      root.render(
        createElement(MonoComposer, {
          sessionId: "mono-ime-test",
          name: "Mono",
          onSubmit,
        }),
      ),
    );
    const field = container.querySelector<HTMLTextAreaElement>(
      'textarea[aria-label="Message Mono"]',
    )!;
    const setValue = Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!;
    act(() => {
      setValue.call(field, "你好");
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    // WebKit ends the composition before keydown, so the Enter that picks a
    // candidate arrives with isComposing false and keyCode 229.
    pressEnter(field, 229);
    expect(onSubmit).not.toHaveBeenCalled();
    pressEnter(field, 13);
    expect(onSubmit).toHaveBeenCalledWith("你好", []);
  } finally {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  }
});
