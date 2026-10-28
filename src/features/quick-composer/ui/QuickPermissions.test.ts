// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { QuickPermissions } from "./QuickPermissions";

let container: HTMLDivElement;
let root: Root;
const onChange = vi.fn();
const onClose = vi.fn();
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  act(() =>
    root.render(
      createElement(QuickPermissions, {
        value: "supervised",
        onChange,
        onClose,
      }),
    ),
  );
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it("shows all four modes and applies the selected permissions", () => {
  const options =
    container.querySelectorAll<HTMLButtonElement>('[role="option"]');
  expect(options).toHaveLength(4);
  expect(options[0].getAttribute("aria-selected")).toBe("true");
  act(() => options[3].click());
  expect(onChange).toHaveBeenCalledWith("full-access");
  expect(onClose).toHaveBeenCalledOnce();
});

it("selects permissions with arrow keys and Enter", () => {
  const menu = container.querySelector('[role="listbox"]')!;
  expect(document.activeElement).toBe(menu);
  act(() =>
    menu.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
    ),
  );
  act(() =>
    menu.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    ),
  );
  expect(onChange).toHaveBeenCalledWith("auto-accept-edits");
  expect(onClose).toHaveBeenCalledOnce();
});

it("dismisses on Escape without changing permissions", () => {
  act(() =>
    container
      .querySelector('[role="listbox"]')!
      .dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      ),
  );
  expect(onChange).not.toHaveBeenCalled();
  expect(onClose).toHaveBeenCalledOnce();
});
