// @vitest-environment happy-dom
import { act, createElement, useRef, useState, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { SavedPrompt, SavedPromptUpsert } from "../model/savedPrompts";
import { requestSavePrompt } from "../model/savedPrompts";
import { SavedPromptMenu } from "./SavedPromptMenu";
import { useSavedPromptMenu } from "./useSavedPromptMenu";
import { PromptsPage } from "./PromptsPage";
import { SavePromptDialogHost } from "./SavePromptDialog";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

let rows: Map<string, SavedPrompt>;
let root: Root;
let container: HTMLDivElement;

function stored(id: string, title: string, changes: Partial<SavedPrompt> = {}) {
  rows.set(id, {
    id,
    title,
    body: `${title} body`,
    pinned: false,
    useCount: 0,
    createdAt: 1,
    updatedAt: 1,
    ...changes,
  });
}

async function flush(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

async function render(node: ReactNode): Promise<void> {
  await act(async () => root.render(node));
  await flush();
}

function setValue(
  element: HTMLTextAreaElement | HTMLInputElement,
  value: string,
): void {
  const proto =
    element instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(element, value);
    element.setSelectionRange?.(value.length, value.length);
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function press(element: HTMLElement, key: string): Promise<void> {
  await act(async () => {
    element.dispatchEvent(
      new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
    );
  });
}

function button(name: string): HTMLButtonElement {
  const found = [...document.querySelectorAll("button")].find(
    (element) =>
      element.textContent?.trim() === name ||
      element.getAttribute("aria-label") === name,
  );
  if (!found) throw new Error(`No button ${name}`);
  return found;
}

async function click(element: HTMLElement): Promise<void> {
  await act(async () => element.click());
  await flush();
}

/** The smallest composer host: a controlled textarea plus the `!` menu. */
function Host(): ReactNode {
  const [value, setText] = useState("Please ");
  const field = useRef<HTMLTextAreaElement>(null);
  const menu = useSavedPromptMenu({
    apply: (next, cursor) => {
      setText(next);
      requestAnimationFrame(() =>
        field.current?.setSelectionRange(cursor, cursor),
      );
    },
  });
  return createElement(
    "div",
    null,
    createElement(SavedPromptMenu, { state: menu }),
    createElement("textarea", {
      ref: field,
      value,
      "aria-label": "Prompt",
      onChange: (event: { currentTarget: HTMLTextAreaElement }) => {
        setText(event.currentTarget.value);
        menu.sync(event.currentTarget);
      },
      onKeyDown: menu.onKeyDown,
    }),
  );
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  rows = new Map();
  vi.mocked(invoke)
    .mockReset()
    .mockImplementation(async (command, args) => {
      if (command === "prompts_list") return [...rows.values()];
      if (command === "prompts_upsert") {
        const input = (args as { prompt: SavedPromptUpsert }).prompt;
        const saved = {
          useCount: 0,
          createdAt: 1,
          ...rows.get(input.id),
          ...input,
          updatedAt: 2,
        };
        rows.set(input.id, saved);
        return saved;
      }
      if (command === "prompts_delete") {
        rows.delete((args as { id: string }).id);
        return undefined;
      }
      if (command === "prompts_mark_used") return undefined;
      throw new Error(`Unexpected ${command}`);
    });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
});

describe("the ! picker", () => {
  it("does not load prompts until ! is typed, then filters, inserts and counts the use", async () => {
    stored("review", "Review this PR", { body: "Review this PR carefully" });
    stored("explain", "Explain simply");
    await render(createElement(Host));
    expect(invoke).not.toHaveBeenCalled();
    const field = container.querySelector("textarea")!;
    setValue(field, "Please !rev");
    await flush();
    const options = container.querySelectorAll('[role="option"]');
    expect([...options].map((option) => option.textContent)).toEqual([
      expect.stringContaining("Review this PR"),
    ]);
    await press(field, "Enter");
    await flush();
    expect(field.value).toBe("Please Review this PR carefully");
    expect(container.querySelector('[role="listbox"]')).toBeNull();
    expect(invoke).toHaveBeenCalledWith("prompts_mark_used", { id: "review" });
  });

  it("closes when nothing matches and on Escape, so ! stays plain text", async () => {
    stored("review", "Review this PR");
    await render(createElement(Host));
    const field = container.querySelector("textarea")!;
    setValue(field, "Please !important");
    await flush();
    expect(container.querySelector("[data-saved-prompt-menu]")).toBeNull();
    setValue(field, "Please !");
    await flush();
    expect(container.querySelector("[data-saved-prompt-menu]")).not.toBeNull();
    await press(field, "Escape");
    expect(container.querySelector("[data-saved-prompt-menu]")).toBeNull();
    setValue(field, "Please !r");
    await flush();
    // Still the dismissed `!`: it stays closed until a new one is typed.
    expect(container.querySelector("[data-saved-prompt-menu]")).toBeNull();
  });

  it("hints where to add prompts when there are none", async () => {
    await render(createElement(Host));
    setValue(container.querySelector("textarea")!, "!");
    await flush();
    expect(container.textContent).toContain("Settings → Prompts");
  });
});

describe("Settings → Prompts", () => {
  it("adds, pins and deletes prompts", async () => {
    await render(createElement(PromptsPage));
    expect(container.textContent).toContain("No prompts yet");
    await click(button("Add prompt"));
    setValue(
      container.querySelector<HTMLInputElement>('input[placeholder^="Shown"]')!,
      "Ship check",
    );
    setValue(
      container.querySelector<HTMLTextAreaElement>("aside textarea")!,
      "Run the tests and lint before pushing",
    );
    await click(button("Save prompt"));
    const [saved] = [...rows.values()];
    expect(saved).toMatchObject({
      title: "Ship check",
      body: "Run the tests and lint before pushing",
      pinned: false,
    });
    expect(container.querySelector("aside")).toBeNull();
    expect(container.textContent).toContain("Ship check");

    await click(button("Pin Ship check"));
    expect(rows.get(saved!.id)?.pinned).toBe(true);

    await click(button("Delete Ship check"));
    await click(button("Delete?"));
    expect(rows.size).toBe(0);
  });

  it("refuses to save an empty prompt", async () => {
    await render(createElement(PromptsPage));
    await click(button("Add prompt"));
    await click(button("Save prompt"));
    expect(container.textContent).toContain("Add the text to insert.");
    expect(invoke).not.toHaveBeenCalledWith(
      "prompts_upsert",
      expect.anything(),
    );
  });
});

describe("Add to Prompts", () => {
  it("opens a prefilled dialog and saves the selected text", async () => {
    await render(createElement(SavePromptDialogHost));
    await act(async () =>
      requestSavePrompt("  Explain the diff\nstep by step  "),
    );
    await flush();
    const title = document.querySelector<HTMLInputElement>(
      'input[placeholder^="Shown"]',
    )!;
    expect(title.value).toBe("Explain the diff");
    await click(button("Save prompt"));
    expect([...rows.values()][0]).toMatchObject({
      title: "Explain the diff",
      body: "Explain the diff\nstep by step",
    });
    expect(document.querySelector('input[placeholder^="Shown"]')).toBeNull();
  });
});
