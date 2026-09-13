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

  it("uses Close as the focus fallback while the modal is locked", () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const onClose = vi.fn();

    act(() => {
      root.render(
        createElement(
          ModalPanel,
          { title: "Example", closeDisabled: true, onClose },
          createElement("button", { type: "button", disabled: true }, "Busy"),
        ),
      );
    });

    const close = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Close"]',
    )!;
    expect(close.disabled).toBe(false);
    expect(close.getAttribute("aria-disabled")).toBe("true");
    expect(document.activeElement).toBe(close);
    close.click();
    expect(onClose).not.toHaveBeenCalled();

    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  it("contains Escape while the modal is locked", () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const onClose = vi.fn();
    const outsideKeyDown = vi.fn();
    document.addEventListener("keydown", outsideKeyDown);

    act(() => {
      root.render(
        createElement(ModalPanel, {
          title: "Example",
          closeDisabled: true,
          onClose,
          children: "Body",
        }),
      );
    });

    const close = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Close"]',
    )!;
    const event = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    });
    act(() => close.dispatchEvent(event));

    expect(event.defaultPrevented).toBe(true);
    expect(outsideKeyDown).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();

    document.removeEventListener("keydown", outsideKeyDown);
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });
});
