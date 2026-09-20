// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../integrations/harness/core/availability", () => ({
  getHarnessAvailabilitySnapshot: () => 0,
  hasProbedHarnessAvailability: () => true,
  isHarnessAvailable: (harness: string) => harness === "grok",
  probeHarnessAvailability: () => Promise.resolve(),
  subscribeHarnessAvailability: () => () => undefined,
}));

vi.mock("../../../integrations/harness/core/registry", () => ({
  refreshHarnessCatalogs: () => Promise.resolve(),
}));

import { SecondOpinionButton } from "./SecondOpinionButton";
import { resetHarnessModelOverlays, setHarnessModels } from "../model/models";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  setHarnessModels("grok", [
    {
      id: "grok:review",
      harness: "grok",
      name: "Review Model",
      settings: [
        {
          id: "effort",
          label: "Reasoning",
          kind: "select",
          value: "high",
          options: [
            { value: "xhigh", label: "Extra High" },
            { value: "high", label: "High" },
            { value: "low", label: "Low" },
          ],
        },
      ],
    },
    {
      id: "grok:quick",
      harness: "grok",
      name: "Quick Model",
    },
  ]);
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

function hover(element: Element) {
  act(() => {
    element.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
  });
}

describe("secondary model target picker", () => {
  it("opens effort beside a hovered model and selects both atomically", () => {
    const onPick = vi.fn();
    act(() =>
      root.render(
        createElement(SecondOpinionButton, {
          from: "cursor",
          onPick,
        }),
      ),
    );

    act(() =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Second opinion"]')!
        .click(),
    );
    const provider = [...document.querySelectorAll('[role="menuitem"]')].find(
      (row) => row.textContent?.includes("Grok Build"),
    )!;
    hover(provider);

    const modelMenu = document.querySelector(
      '[role="menu"][aria-label="Grok Build models"]',
    )!;
    const model = [...modelMenu.querySelectorAll('[role="menuitem"]')].find(
      (row) => row.textContent?.includes("Review Model"),
    )!;
    hover(model);

    expect(onPick).not.toHaveBeenCalled();
    const effortMenu = document.querySelector(
      '[role="menu"][aria-label="Review Model effort"]',
    )!;
    expect(effortMenu).toBeTruthy();
    const extraHigh = [
      ...effortMenu.querySelectorAll<HTMLButtonElement>(
        '[role="menuitemradio"]',
      ),
    ].find((option) => option.textContent === "Extra High")!;
    act(() => extraHigh.click());

    expect(onPick).toHaveBeenCalledWith({
      harness: "grok",
      model: "grok:review",
      modelSettings: { effort: "xhigh" },
    });
  });

  it("selects a model without effort directly", () => {
    const onPick = vi.fn();
    act(() =>
      root.render(
        createElement(SecondOpinionButton, {
          from: "cursor",
          onPick,
        }),
      ),
    );

    act(() =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Second opinion"]')!
        .click(),
    );
    const provider = [...document.querySelectorAll('[role="menuitem"]')].find(
      (row) => row.textContent?.includes("Grok Build"),
    )!;
    hover(provider);
    const quick = [
      ...document.querySelectorAll<HTMLButtonElement>(
        '[role="menu"][aria-label="Grok Build models"] [role="menuitem"]',
      ),
    ].find((row) => row.textContent?.includes("Quick Model"))!;
    act(() => quick.click());

    expect(onPick).toHaveBeenCalledWith({
      harness: "grok",
      model: "grok:quick",
      modelSettings: {},
    });
  });
});
