// @vitest-environment happy-dom
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/ui/Popover", () => ({
  Popover: ({
    children,
    role,
    tabIndex,
    onKeyDown,
  }: {
    children: ReactNode;
    role?: string;
    tabIndex?: number;
    onKeyDown?: React.KeyboardEventHandler<HTMLDivElement>;
  }) => createElement("div", { role, tabIndex, onKeyDown }, children),
}));

import { AccessPicker } from "./AccessPicker";
import { resetHarnessModelOverlays, setHarnessModels } from "../model/models";
import type { HarnessId, RuntimeMode } from "../model/session";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  resetHarnessModelOverlays();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  resetHarnessModelOverlays();
  container.remove();
  vi.unstubAllGlobals();
});

function render(
  harness: HarnessId,
  model: string,
  value: RuntimeMode,
  onChange: (mode: RuntimeMode) => void = () => undefined,
) {
  act(() =>
    root.render(
      createElement(AccessPicker, { harness, model, value, onChange }),
    ),
  );
}

function trigger() {
  return container.querySelector<HTMLButtonElement>(
    "[data-access-picker-trigger]",
  )!;
}

function open() {
  act(() => trigger().click());
}

function options() {
  return [...container.querySelectorAll("[role=option]")].map(
    (el) => el.querySelector("span span")?.textContent,
  );
}

function press(key: string) {
  act(() => {
    container
      .querySelector("[role=listbox]")!
      .dispatchEvent(
        new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
      );
  });
}

const claude = (supportsAuto?: boolean) => [
  {
    id: "claude:m",
    harness: "claude" as const,
    name: "M",
    ...(supportsAuto === undefined ? {} : { supportsAuto }),
  },
];

describe("AccessPicker", () => {
  it("lists Auto only for models that support it", () => {
    render("claude", "claude:sonnet-5", "supervised");
    open();
    expect(options()).toContain("Auto");

    render("cursor", "cursor:default", "supervised");
    expect(options()).not.toContain("Auto");
  });

  it("drops Auto when the model or harness changes", () => {
    render("claude", "claude:sonnet-5", "supervised");
    open();
    expect(options()).toContain("Auto");
    render("opencode", "opencode:x", "supervised");
    expect(options()).not.toContain("Auto");
    render("codex", "codex:gpt-5", "supervised");
    expect(options()).toContain("Auto");
  });

  it("removes Auto when a late catalog reports no support", () => {
    render("claude", "claude:m", "supervised");
    open();
    expect(options()).toContain("Auto");
    act(() => setHarnessModels("claude", claude(false)));
    expect(options()).not.toContain("Auto");
  });

  it("steps down to the previous mode through onChange", () => {
    const onChange = vi.fn();
    render("cursor", "cursor:default", "auto", onChange);
    expect(onChange).toHaveBeenCalledExactlyOnceWith("auto-accept-edits");
    expect(trigger().getAttribute("aria-label")).toBe("Auto-accept edits");
  });

  it("does not call onChange when the mode is offered", () => {
    const onChange = vi.fn();
    render("claude", "claude:sonnet-5", "auto", onChange);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("selects the right option with Enter after the options change", () => {
    setHarnessModels("claude", claude(true));
    const onChange = vi.fn();
    render("claude", "claude:m", "full-access", onChange);
    open();
    expect(options()).toHaveLength(4);
    act(() => setHarnessModels("claude", claude(false)));
    expect(options()).toHaveLength(3);
    press("Enter");
    expect(onChange).toHaveBeenCalledExactlyOnceWith("full-access");
  });
});
