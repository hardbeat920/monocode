// @vitest-environment happy-dom
import { act, createElement, type CSSProperties, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/harness/availability", () => ({
  getHarnessAvailabilitySnapshot: () => 0,
  hasProbedHarnessAvailability: () => true,
  isHarnessAvailable: () => true,
  probeHarnessAvailability: () => Promise.resolve(),
  subscribeHarnessAvailability: () => () => undefined,
}));

vi.mock("../lib/harness/registry", () => ({
  refreshHarnessCatalogs: () => Promise.resolve(),
}));

vi.mock("./Popover", () => ({
  Popover: ({
    children,
    role,
    className,
    tabIndex,
    onKeyDown,
    ...props
  }: {
    children: ReactNode;
    role?: string;
    className?: string;
    tabIndex?: number;
    onKeyDown?: React.KeyboardEventHandler<HTMLDivElement>;
    minHeight?: number;
    maxHeight?: number;
    style?: CSSProperties;
    anchor?: unknown;
    side?: string;
    "aria-label"?: string;
    "data-model-picker"?: boolean;
  }) =>
    createElement(
      "div",
      {
        role,
        className,
        tabIndex,
        onKeyDown,
        style: props.style,
        "data-min-height": props.minHeight,
        "data-max-height": props.maxHeight,
        "data-anchor-element":
          props.anchor instanceof HTMLElement ||
          (typeof props.anchor === "object" &&
            props.anchor != null &&
            "current" in props.anchor &&
            props.anchor.current instanceof HTMLElement)
            ? "true"
            : "false",
        "data-side": props.side,
        "aria-label": props["aria-label"],
        "data-model-picker": props["data-model-picker"] ? "" : undefined,
      },
      children,
    ),
}));

import { ModelPicker } from "./ModelPicker";
import { saveRecentModelChoice } from "../lib/models";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function hover(element: Element) {
  act(() => {
    element.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
  });
}

function contextMenu(element: Element) {
  act(() => {
    element.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        clientX: 120,
        clientY: 80,
      }),
    );
  });
}

function keyDown(target: EventTarget, key: string) {
  act(() => {
    target.dispatchEvent(
      new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
    );
  });
}

describe("model picker", () => {
  it("shows the model name while keeping effort in the combined picker", () => {
    const onChange = vi.fn();
    const onSettingsChange = vi.fn();
    act(() =>
      root.render(
        createElement(ModelPicker, {
          harness: "grok",
          model: "grok:grok-4.6",
          values: { effort: "high" },
          onChange,
          onSettingsChange,
        }),
      ),
    );

    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-haspopup="menu"]',
    )!;
    expect(trigger.textContent).toBe("Grok 4.6");
    expect(trigger.querySelector("svg")).not.toBeNull();

    act(() => trigger.click());
    const modelRow = [
      ...container.querySelectorAll<HTMLButtonElement>("button"),
    ].find((button) => button.textContent?.startsWith("Model"))!;
    expect(modelRow.textContent).toContain("Grok 4.6");
    expect(modelRow.querySelectorAll("svg")).toHaveLength(2);

    hover(modelRow);
    const modelFlyout = container.querySelector<HTMLElement>(
      '[role="dialog"][aria-label="Models"]',
    )!;
    expect(modelFlyout.style.height).toBe("332px");
    expect(modelFlyout.dataset.minHeight).toBe("334");
    expect(modelFlyout.dataset.maxHeight).toBe("334");
    expect(
      container.querySelector('[role="tablist"][aria-orientation="vertical"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[role="tab"][aria-label="Favorites"]'),
    ).not.toBeNull();
    const grokTab = container.querySelector<HTMLButtonElement>(
      '[role="tab"][aria-label="Grok Build"]',
    )!;
    expect(grokTab.className).toContain("rounded-md");
    expect(grokTab.className).not.toContain("transition");
    hover(grokTab);
    expect(grokTab.getAttribute("aria-selected")).toBe("true");
    expect(
      [...container.querySelectorAll('[role="option"]')].some(
        (option) => option.textContent === "Grok 4.6",
      ),
    ).toBe(true);
    const selectedOption = container.querySelector<HTMLButtonElement>(
      '[role="option"][aria-selected="true"]',
    )!;
    const selectedRow = selectedOption.parentElement!;
    const favoriteButton = selectedRow.querySelector<HTMLButtonElement>(
      'button[aria-label="Add to favorites"]',
    )!;
    expect(favoriteButton.className).toContain("opacity-0");
    expect(favoriteButton.className).toContain("group-hover:opacity-100");
    expect(selectedRow.lastElementChild?.querySelector("svg")).not.toBeNull();
    const unselectedOption = container.querySelector<HTMLButtonElement>(
      '[role="option"][aria-selected="false"]',
    )!;
    const unselectedRow = unselectedOption.parentElement!;
    expect(unselectedRow.lastElementChild?.getAttribute("aria-label")).toBe(
      "Add to favorites",
    );
    expect(
      container.querySelector('input[aria-label="Search models"]'),
    ).not.toBeNull();

    const effortRow = [
      ...container.querySelectorAll<HTMLButtonElement>("button"),
    ].find((button) => button.textContent?.startsWith("Effort"))!;
    hover(effortRow);
    const extraHigh = [
      ...container.querySelectorAll<HTMLButtonElement>("button"),
    ].find((button) => button.textContent === "Extra High")!;
    act(() => extraHigh.click());

    expect(onSettingsChange).toHaveBeenCalledWith({ effort: "xhigh" });
    expect(onChange).not.toHaveBeenCalled();
  });

  it("returns to the selected model's harness when reopened", () => {
    act(() =>
      root.render(
        createElement(ModelPicker, {
          harness: "grok",
          model: "grok:grok-4.6",
          values: { effort: "high" },
          onChange: vi.fn(),
          onSettingsChange: vi.fn(),
        }),
      ),
    );

    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-haspopup="menu"]',
    )!;
    act(() => trigger.click());
    let modelRow = [
      ...container.querySelectorAll<HTMLButtonElement>("button"),
    ].find((button) => button.textContent?.startsWith("Model"))!;
    hover(modelRow);

    const openCodeTab = container.querySelector<HTMLButtonElement>(
      '[role="tab"][aria-label="OpenCode"]',
    )!;
    hover(openCodeTab);
    expect(openCodeTab.getAttribute("aria-selected")).toBe("true");

    act(() => trigger.click());
    act(() => trigger.click());
    modelRow = [
      ...container.querySelectorAll<HTMLButtonElement>("button"),
    ].find((button) => button.textContent?.startsWith("Model"))!;
    hover(modelRow);

    expect(
      container
        .querySelector('[role="tab"][aria-label="Grok Build"]')
        ?.getAttribute("aria-selected"),
    ).toBe("true");
  });

  it("quick-switches between recently used models on right-click", () => {
    saveRecentModelChoice("claude", "claude:opus-5");
    saveRecentModelChoice("cursor", "cursor:composer-2.5");
    const onChange = vi.fn();
    act(() =>
      root.render(
        createElement(ModelPicker, {
          harness: "grok",
          model: "grok:grok-4.6",
          values: { effort: "high" },
          hotkeys: true,
          onChange,
          onSettingsChange: vi.fn(),
        }),
      ),
    );

    const trigger = container.querySelector<HTMLButtonElement>(
      'button[aria-haspopup="menu"]',
    )!;
    contextMenu(trigger);

    const recentMenu = container.querySelector<HTMLElement>(
      '[role="menu"][aria-label="Recently used models"]',
    )!;
    expect(recentMenu.dataset.anchorElement).toBe("true");
    expect(recentMenu.dataset.side).toBe("top");
    const recentItems = [
      ...recentMenu.querySelectorAll<HTMLButtonElement>(
        '[role="menuitemradio"]',
      ),
    ];
    expect(recentItems).toHaveLength(3);
    const grokModel = recentItems.find((item) =>
      item.textContent?.includes("Grok 4.6"),
    )!;
    const claudeModel = recentItems.find((item) =>
      item.textContent?.includes("Opus 5"),
    )!;
    const cursorModel = recentItems.find((item) =>
      item.textContent?.includes("Composer 2.5"),
    )!;
    expect(cursorModel.textContent).toContain("Cursor");
    expect(cursorModel.querySelector("svg")).not.toBeNull();

    // The composer can retain focus after opening its toolbar menu. Recent
    // model navigation still needs to own these keys in that state.
    trigger.focus();
    expect(grokModel.className).toContain("bg-content/10");
    keyDown(trigger, "ArrowUp");
    expect(claudeModel.className).toContain("bg-content/10");
    keyDown(trigger, "ArrowDown");
    expect(grokModel.className).toContain("bg-content/10");
    keyDown(trigger, "ArrowDown");
    expect(cursorModel.className).toContain("bg-content/10");

    keyDown(trigger, "Enter");
    expect(onChange).toHaveBeenCalledWith("cursor", "cursor:composer-2.5");
    expect(
      container.querySelector(
        '[role="menu"][aria-label="Recently used models"]',
      ),
    ).toBeNull();

    act(() => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: ".",
          code: "Period",
          metaKey: true,
          bubbles: true,
        }),
      );
    });
    expect(
      container.querySelector(
        '[role="menu"][aria-label="Recently used models"]',
      ),
    ).not.toBeNull();
    expect(
      container.querySelector('[role="menu"][aria-label="Model and effort"]'),
    ).toBeNull();
  });
});
