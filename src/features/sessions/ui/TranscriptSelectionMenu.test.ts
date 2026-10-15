// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TranscriptSelectionMenu } from "./TranscriptSelectionMenu";

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

describe("TranscriptSelectionMenu", () => {
  it("keeps selected text available when saving a note fails", async () => {
    let rejectSave!: (reason: Error) => void;
    const onDismiss = vi.fn();
    const onAddToNotes = vi.fn(
      () =>
        new Promise<void>((_resolve, reject) => {
          rejectSave = reject;
        }),
    );
    act(() =>
      root.render(
        createElement(TranscriptSelectionMenu, {
          selection: { text: "Keep this", rect: new DOMRect(10, 20, 100, 20) },
          onAddToNotes,
          onDismiss,
        }),
      ),
    );
    const button = document.querySelector<HTMLButtonElement>(
      '[role="toolbar"] button',
    )!;
    await act(async () => button.click());
    expect(onDismiss).not.toHaveBeenCalled();
    expect(button.disabled).toBe(true);
    await act(async () => rejectSave(new Error("Disk full")));
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      "Disk full",
    );
    expect(button.disabled).toBe(false);
    onAddToNotes.mockResolvedValue();
    await act(async () => button.click());
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("offers the selected text to both chat and notes", () => {
    const onAddToChat = vi.fn();
    const onAddToNotes = vi.fn();
    const onDismiss = vi.fn();
    act(() =>
      root.render(
        createElement(TranscriptSelectionMenu, {
          selection: {
            text: "A useful link",
            rect: new DOMRect(10, 20, 100, 20),
          },
          onAddToChat,
          onAddToNotes,
          onDismiss,
        }),
      ),
    );

    const toolbar = document.querySelector(
      '[role="toolbar"][aria-label="Selected text actions"]',
    );
    expect(toolbar?.textContent).toContain("Add to chat");
    expect(toolbar?.textContent).toContain("Add to notes");

    const notes = Array.from(toolbar?.querySelectorAll("button") ?? []).find(
      (button) => button.textContent?.includes("Add to notes"),
    );
    act(() => notes?.click());

    expect(onAddToNotes).toHaveBeenCalledWith("A useful link");
    expect(onAddToChat).not.toHaveBeenCalled();
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("asks for a note before adding the selection to chat", async () => {
    const onAddToChat = vi.fn();
    const onDismiss = vi.fn();
    const range = document.createRange();
    const selection = {
      text: "A useful link",
      rect: new DOMRect(10, 20, 100, 20),
      range,
    };
    const show = (current: typeof selection | null) =>
      act(() =>
        root.render(
          createElement(TranscriptSelectionMenu, {
            selection: current,
            onAddToChat,
            onDismiss,
          }),
        ),
      );
    show(selection);
    const chat = document.querySelector<HTMLButtonElement>(
      '[role="toolbar"] button',
    )!;
    act(() => chat.click());
    expect(onAddToChat).not.toHaveBeenCalled();

    // The page selection clears once the note field takes focus.
    show(null);
    const field = document.querySelector<HTMLTextAreaElement>(
      '[role="dialog"] textarea',
    )!;
    await act(async () => {
      await new Promise((resolve) => requestAnimationFrame(resolve));
    });
    expect(document.activeElement).toBe(field);
    expect(field).not.toBeNull();
    const set = Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!;
    act(() => {
      set.call(field, "this part is wrong");
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    act(() => {
      field.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });
    expect(onAddToChat).toHaveBeenCalledWith(
      "A useful link",
      "this part is wrong",
      range,
    );
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it("adds the selection with an empty note when none is written", () => {
    const onAddToChat = vi.fn();
    act(() =>
      root.render(
        createElement(TranscriptSelectionMenu, {
          selection: { text: "Plain", rect: new DOMRect(10, 20, 100, 20) },
          onAddToChat,
          onDismiss: vi.fn(),
        }),
      ),
    );
    act(() =>
      document
        .querySelector<HTMLButtonElement>('[role="toolbar"] button')!
        .click(),
    );
    const add = [...document.querySelectorAll("button")].find(
      (button) => button.textContent === "Add",
    )!;
    act(() => add.click());
    expect(onAddToChat).toHaveBeenCalledWith("Plain", "", undefined);
  });
});
