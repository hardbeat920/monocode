// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Composer } from "./Composer";
import {
  composeChatContext,
  splitChatContext,
  type ChatContextItem,
} from "../model/chatContext";
import type { ComposerInsertRequest } from "../model/quoteDraft";

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));

const code: ChatContextItem = {
  kind: "code",
  path: "src/app/App.tsx",
  startLine: 12,
  endLine: 40,
};
const quote: ChatContextItem = { kind: "quote", text: "Use a cookie." };

describe("Composer context chips", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  function props(overrides: Record<string, unknown> = {}) {
    return {
      focused: false,
      harness: "claude" as const,
      model: "claude-sonnet",
      runtimeMode: "supervised" as const,
      executionCwd: "~",
      hideTopBar: true,
      onFocus: vi.fn(),
      onCwdChange: vi.fn(),
      onModelChange: vi.fn(),
      onRuntimeModeChange: vi.fn(),
      onSubmit: vi.fn(),
      ...overrides,
    };
  }

  async function render(overrides: Record<string, unknown> = {}) {
    await act(async () =>
      root.render(createElement(Composer, props(overrides))),
    );
  }

  const chips = () =>
    [...container.querySelectorAll("[data-chat-context-chip]")].map(
      (chip) => chip.getAttribute("data-chat-context-chip"),
    );
  const send = () =>
    act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[aria-label="Send"]')!
        .click(),
    );

  it("shows an inserted chip instead of pasting text and sends it after the message", async () => {
    const onSubmit = vi.fn();
    const onInsertRequestConsumed = vi.fn();
    const request: ComposerInsertRequest = { id: 1, kind: "context", item: code };
    await render({
      initialDraft: "Explain this",
      insertRequest: request,
      onInsertRequestConsumed,
      onSubmit,
    });

    const textarea = container.querySelector("textarea")!;
    expect(textarea.value).toBe("Explain this");
    expect(chips()).toEqual(["code"]);
    expect(container.textContent).toContain("App.tsx");
    expect(container.textContent).toContain("L12-40");
    expect(onInsertRequestConsumed).toHaveBeenCalledWith(1);

    await send();

    expect(onSubmit.mock.calls[0][0]).toBe(
      composeChatContext("Explain this", [code]),
    );
    expect(chips()).toEqual([]);
  });

  it("sends a message that only has context", async () => {
    const onSubmit = vi.fn();
    await render({ initialDraft: composeChatContext("", [quote]), onSubmit });

    const textarea = container.querySelector("textarea")!;
    expect(textarea.value).toBe("");
    expect(textarea.placeholder).toBe("Add a message, or send…");
    expect(chips()).toEqual(["quote"]);

    await send();

    expect(splitChatContext(onSubmit.mock.calls[0][0])).toEqual({
      text: "",
      items: [quote],
    });
  });

  it("removes a chip and keeps the parent draft in sync", async () => {
    let parentDraft = composeChatContext("Hi", [code, quote]);
    const onDraftChange = vi.fn((text: string) => {
      parentDraft = text;
    });
    await render({ initialDraft: parentDraft, onDraftChange });

    expect(chips()).toEqual(["code", "quote"]);
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>(
          '[data-chat-context-chip="code"] button[title="Remove"]',
        )!
        .click(),
    );

    expect(chips()).toEqual(["quote"]);
    expect(parentDraft).toBe(composeChatContext("Hi", [quote]));
  });

  it("restores chips when the app rejects a turn", async () => {
    await render({
      initialDraft: composeChatContext("Blocked", [code]),
      onSubmit: () => false,
    });

    await send();

    expect(container.querySelector("textarea")!.value).toBe("Blocked");
    expect(chips()).toEqual(["code"]);
  });

  it("saves chips with a draft message", async () => {
    const onSaveDraft = vi.fn(() => true);
    await render({
      initialDraft: composeChatContext("Later", [code]),
      canSaveDraft: true,
      onSaveDraft,
    });

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>(
          'button[aria-label="Add files or choose a mode"]',
        )!
        .click(),
    );
    await act(async () =>
      [...document.querySelectorAll<HTMLButtonElement>("button")]
        .find((button) => button.textContent?.includes("Save this message"))!
        .click(),
    );
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Save draft"]')!
        .click(),
    );

    expect(onSaveDraft).toHaveBeenCalledWith(
      composeChatContext("Later", [code]),
      [],
    );
    expect(chips()).toEqual([]);
  });

  it("opens the file at the selected line when a code chip is clicked", async () => {
    const onOpenFile = vi.fn();
    await render({ initialDraft: composeChatContext("", [code]), onOpenFile });

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>(
          '[data-chat-context-chip="code"] button:not([title="Remove"])',
        )!
        .click(),
    );

    expect(onOpenFile).toHaveBeenCalledWith("src/app/App.tsx", { line: 12 });
  });
});
