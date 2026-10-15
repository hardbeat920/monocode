// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ChatReferencesPill } from "./ChatReferencesPill";
import { newChatReference } from "../model/chatReferences";

let container: HTMLDivElement;
let root: Root;
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
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

const first = newChatReference("First passage", "check this");
const second = newChatReference("Second passage", "");

function render(onChange = vi.fn()) {
  act(() =>
    root.render(
      createElement(ChatReferencesPill, {
        references: [first, second],
        onChange,
      }),
    ),
  );
  return onChange;
}
function type(el: HTMLTextAreaElement, value: string) {
  const set = Object.getOwnPropertyDescriptor(
    HTMLTextAreaElement.prototype,
    "value",
  )!.set!;
  set.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

it("counts every reference in one pill and shows them on hover", () => {
  render();
  expect(container.textContent).toContain("2 annotations");
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  act(() =>
    container
      .querySelector("[data-chat-references-pill]")!
      .dispatchEvent(new MouseEvent("mouseover", { bubbles: true })),
  );
  const dialog = document.querySelector('[role="dialog"]')!;
  expect(dialog.textContent).toContain("First passage");
  expect(dialog.textContent).toContain("check this");
  expect(dialog.textContent).toContain("Second passage");
  expect(dialog.textContent).toContain("Add a note");
});

it("edits one note and removes one reference from the list", () => {
  const onChange = render();
  act(() =>
    container
      .querySelector<HTMLButtonElement>("[data-chat-references-pill] button")!
      .click(),
  );
  const dialog = document.querySelector('[role="dialog"]')!;
  const noteButton = [...dialog.querySelectorAll("button")].find((button) =>
    button.textContent?.includes("Add a note"),
  )!;
  act(() => noteButton.click());
  const field = dialog.querySelector("textarea")!;
  act(() => type(field, "second thought"));
  act(() => {
    field.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
  });
  expect(onChange).toHaveBeenLastCalledWith([
    first,
    { ...second, note: "second thought" },
  ]);

  act(() =>
    dialog
      .querySelector<HTMLButtonElement>('[aria-label="Remove annotation 1"]')!
      .click(),
  );
  expect(onChange).toHaveBeenLastCalledWith([second]);
});

it("removes everything from the pill's own X", () => {
  const onChange = render();
  act(() =>
    container
      .querySelector<HTMLButtonElement>(
        '[aria-label="Remove all annotations"]',
      )!
      .click(),
  );
  expect(onChange).toHaveBeenCalledWith([]);
});

it("starts closed again after the last annotation is removed", () => {
  const onChange = vi.fn();
  const show = (references: (typeof first)[]) =>
    act(() =>
      root.render(createElement(ChatReferencesPill, { references, onChange })),
    );
  show([first]);
  act(() =>
    container
      .querySelector<HTMLButtonElement>("[data-chat-references-pill] button")!
      .click(),
  );
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  show([]);
  show([second]);
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});
