// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { LAYER } from "../lib/layers";
import { SearchableSelect } from "./SearchableSelect";

let root: Root;
let container: HTMLDivElement;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  document.body
    .querySelectorAll("[data-dialog-popover]")
    .forEach((element) => element.parentElement?.remove());
  vi.unstubAllGlobals();
});

it("raises menus above a containing modal by default", async () => {
  await act(async () => {
    root.render(
      createElement(
        "div",
        { role: "dialog" },
        createElement(SearchableSelect, {
          label: "Agent",
          value: "codex",
          options: [
            { value: "codex", label: "Codex" },
            { value: "claude", label: "Claude" },
          ],
          onChange: vi.fn(),
        }),
      ),
    );
  });

  const trigger = container.querySelector<HTMLButtonElement>("button")!;
  await act(async () => trigger.click());

  const menu = document.body.querySelector<HTMLElement>(
    '[aria-label="Agent options"]',
  );
  expect(menu).not.toBeNull();
  expect(menu!.parentElement!.style.zIndex).toBe(String(LAYER.dialogPopover));
});

it("does not scroll the page when a compact menu opens", async () => {
  const scrollIntoView = vi.spyOn(HTMLElement.prototype, "scrollIntoView");
  await act(async () => {
    root.render(
      createElement(SearchableSelect, {
        label: "Timeout",
        value: "60",
        variant: "pill",
        searchable: false,
        options: [
          { value: "30", label: "30 sec" },
          { value: "60", label: "1 min" },
          { value: "120", label: "2 min" },
          { value: "300", label: "5 min" },
        ],
        onChange: vi.fn(),
      }),
    );
  });

  const trigger = container.querySelector<HTMLButtonElement>("button")!;
  await act(async () => trigger.click());
  await act(async () => {
    await Promise.resolve();
  });

  expect(scrollIntoView).not.toHaveBeenCalled();
  const option = document.body.querySelector('[role="option"]');
  expect(option?.className).toContain("h-7");
});

it("opens a bar menu as wide as its row, names the selection on hover and picks a searched option", async () => {
  const onChange = vi.fn();
  const name = "MonoCode desktop app — Sonar connector branch filters";
  await act(async () => {
    root.render(
      createElement(SearchableSelect, {
        label: "Sonar project",
        value: "app",
        variant: "bar",
        options: [
          { value: "app", label: name },
          { value: "api", label: "Payments API", keywords: "org_payments" },
        ],
        onChange,
      }),
    );
  });

  const trigger = container.querySelector<HTMLButtonElement>("button")!;
  expect(trigger.title).toBe(name);
  vi.spyOn(trigger.parentElement!, "getBoundingClientRect").mockReturnValue(
    DOMRect.fromRect({ width: 400, height: 28 }),
  );
  await act(async () => trigger.click());
  const search =
    document.body.querySelector<HTMLInputElement>('[role="combobox"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(search, "org_pay");
    search.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const options =
    document.body.querySelectorAll<HTMLButtonElement>('[role="option"]');
  expect(options).toHaveLength(1);
  await act(async () => options[0]!.click());
  expect(onChange).toHaveBeenCalledWith("api");
});
