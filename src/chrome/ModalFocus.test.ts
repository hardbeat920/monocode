// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { ModalPanel } from "./Modal";

describe("ModalPanel focus", () => {
  it("wraps keyboard focus inside the dialog", () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        createElement(
          ModalPanel,
          { title: "Example", onClose: vi.fn() },
          createElement("button", { type: "button" }, "Action"),
        ),
      );
    });

    const close = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Close"]',
    );
    const action = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "Action",
    );
    expect(close).toBeTruthy();
    expect(action).toBeTruthy();

    action!.focus();
    act(() => {
      action!.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Tab",
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(document.activeElement).toBe(close);

    close!.focus();
    act(() => {
      close!.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Tab",
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(document.activeElement).toBe(action);

    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  it("restores focus to the trigger when the dialog unmounts", () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const trigger = document.createElement("button");
    const container = document.createElement("div");
    document.body.append(trigger, container);
    trigger.focus();
    const root = createRoot(container);

    act(() => {
      root.render(
        createElement(ModalPanel, {
          title: "Example",
          onClose: vi.fn(),
          children: "Body",
        }),
      );
    });
    expect(document.activeElement).not.toBe(trigger);

    act(() => root.unmount());
    expect(document.activeElement).toBe(trigger);

    trigger.remove();
    container.remove();
    vi.unstubAllGlobals();
  });
});
