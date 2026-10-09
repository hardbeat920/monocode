// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Composer } from "./Composer";
import { copyMessage } from "../../../platform/tauri/clipboard";

const { invoke } = vi.hoisted(() => ({
  invoke: vi.fn(async () => []),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));

it("pastes copied message text at the selection together with its attachment", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const submit = vi.fn();
  let written: ClipboardItem[] = [];
  vi.spyOn(navigator.clipboard, "write").mockImplementation(async (items) => {
    written = items;
  });
  try {
    await copyMessage("See image", [
      {
        id: "a",
        name: "shot.png",
        mimeType: "image/png",
        kind: "image",
        size: 3,
        data: "YWJj",
      },
    ]);
    const html = await (await written[0].getType("text/html")).text();
    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: false,
          harness: "codex",
          model: "",
          runtimeMode: "supervised",
          executionCwd: "~",
          initialDraft: "Before replace after",
          hideTopBar: true,
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit: submit,
        }),
      ),
    );
    const textarea = container.querySelector("textarea")!;
    textarea.setSelectionRange(7, 14);
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", {
      value: {
        getData: (type: string) => (type === "text/html" ? html : "See image"),
        files: [],
        items: [],
      },
    });
    await act(async () => {
      textarea.dispatchEvent(event);
      await new Promise((resolve) => setTimeout(resolve, 30));
    });
    expect(textarea.value).toBe("Before See image after");
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[aria-label="Send"]')!
        .click(),
    );
    expect(submit.mock.calls[0][0]).toBe("Before See image after");
    expect(submit.mock.calls[0][1][0]).toMatchObject({
      name: "shot.png",
      data: "YWJj",
    });
  } finally {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});

it("leaves a pasted GitHub pull request URL as editable text", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const submit = vi.fn();
  const onDraftChange = vi.fn();
  const url = "https://github.com/hardbeat920/monocode/pull/318";
  const props = {
    focused: false,
    harness: "codex" as const,
    model: "",
    runtimeMode: "supervised" as const,
    executionCwd: "/repo",
    initialDraft: "What is this  explain in simple terms",
    hideTopBar: true,
    onFocus: vi.fn(),
    onCwdChange: vi.fn(),
    onModelChange: vi.fn(),
    onRuntimeModeChange: vi.fn(),
    onSubmit: submit,
    onDraftChange,
  };
  try {
    await act(async () => root.render(createElement(Composer, props)));
    const textarea = container.querySelector("textarea")!;
    const insertionPoint = "What is this ".length;
    textarea.setSelectionRange(insertionPoint, insertionPoint);
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", {
      value: {
        getData: (type: string) => (type === "text/plain" ? url : ""),
        files: [],
        items: [],
      },
    });

    await act(async () => textarea.dispatchEvent(event));

    expect(event.defaultPrevented).toBe(false);
    expect(container.querySelector("[data-composer-link-chip]")).toBeNull();

    await act(async () => {
      textarea.setRangeText(url, insertionPoint, insertionPoint, "end");
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const expected = `What is this ${url} explain in simple terms`;
    expect(textarea.value).toBe(expected);
    expect(onDraftChange).toHaveBeenLastCalledWith(expected);

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[aria-label="Send"]')!
        .click(),
    );
    expect(submit).toHaveBeenCalledWith(expected, [], { intent: "default" });
  } finally {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});

describe("large plain-text pastes", () => {
  let container: HTMLDivElement;
  let root: Root;
  let submit: ReturnType<typeof vi.fn>;
  let openFile: ReturnType<typeof vi.fn>;

  const draw = (props: Record<string, unknown> = {}) => {
    act(() => {
      root.render(
        createElement(Composer, {
          focused: false,
          harness: "codex",
          model: "",
          runtimeMode: "supervised",
          executionCwd: "/repo",
          hideTopBar: true,
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit: submit,
          onOpenFile: openFile,
          ...props,
        }),
      );
    });
    return container.querySelector("textarea")!;
  };

  const paste = (target: HTMLTextAreaElement, text: string) => {
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", {
      value: {
        getData: (type: string) => (type === "text/plain" ? text : ""),
        files: [],
        items: [],
      },
    });
    act(() => target.dispatchEvent(event));
    return event;
  };

  const settleUntil = async (check: () => boolean) => {
    for (let waited = 0; waited < 2_000; waited += 10) {
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 10));
      });
      if (check()) return;
    }
    throw new Error("Timed out waiting for pasted text attachment.");
  };

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    invoke.mockReset();
    invoke.mockImplementation(async (command: string) =>
      command === "write_attachment" ? "/tmp/pasted-text.txt" : [],
    );
    submit = vi.fn();
    openFile = vi.fn();
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("leaves text at or below 2,000 characters inline", async () => {
    const text = "x".repeat(2_000);
    const textarea = draw({ initialDraft: "Before " });
    textarea.setSelectionRange(7, 7);

    const event = paste(textarea, text);

    expect(event.defaultPrevented).toBe(false);
    await act(async () => {
      textarea.setRangeText(text, 7, 7, "end");
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(textarea.value).toBe(`Before ${text}`);
    expect(invoke).not.toHaveBeenCalledWith(
      "write_attachment",
      expect.anything(),
    );
  });

  it("turns text above 2,000 characters into one attachment", async () => {
    const text = "x".repeat(2_001);
    const textarea = draw({ initialDraft: "Keep" });

    const event = paste(textarea, text);
    await settleUntil(
      () =>
        container.querySelectorAll('[aria-label="Remove pasted-text.txt"]')
          .length === 1,
    );

    expect(event.defaultPrevented).toBe(true);
    expect(textarea.value).toBe("Keep");
    expect(invoke).toHaveBeenCalledWith("write_attachment", {
      name: "pasted-text.txt",
      data: btoa(text),
    });
    act(() =>
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Open pasted-text.txt"]',
        )!
        .click(),
    );
    expect(openFile).toHaveBeenCalledWith("/tmp/pasted-text.txt", undefined, {
      exact: true,
    });

    act(() =>
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Remove pasted-text.txt"]',
        )!
        .click(),
    );
    expect(
      container.querySelector('[aria-label="Remove pasted-text.txt"]'),
    ).toBeNull();

    paste(textarea, text);
    await settleUntil(
      () =>
        container.querySelector('[aria-label="Remove pasted-text.txt"]') !==
        null,
    );
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[aria-label="Send"]')!
        .click(),
    );
    expect(submit.mock.calls[0][1]).toEqual([
      expect.objectContaining({
        name: "pasted-text.txt",
        mimeType: "text/plain",
        path: "/tmp/pasted-text.txt",
      }),
    ]);
  });

  it("leaves large text inline when attachments are unsupported", () => {
    const text = "x".repeat(2_001);
    const textarea = draw({ harness: "fx" });

    const event = paste(textarea, text);

    expect(event.defaultPrevented).toBe(false);
    act(() => {
      textarea.setRangeText(text, 0, 0, "end");
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(textarea.value).toBe(text);
    expect(invoke).not.toHaveBeenCalledWith(
      "write_attachment",
      expect.anything(),
    );
  });

  it("does not offer local editing for a remote attachment", async () => {
    const textarea = draw({
      remoteSession: true,
      remoteFeatures: { attachments: true, plan: false, draft: false },
    });

    paste(textarea, "x".repeat(2_001));
    await settleUntil(
      () =>
        container.querySelector('[aria-label="Remove pasted-text.txt"]') !==
        null,
    );

    expect(
      container.querySelector('[aria-label="Open pasted-text.txt"]'),
    ).toBeNull();
  });

  it("drops a large paste that finishes after the draft is reset", async () => {
    let release: (() => void) | undefined;
    invoke.mockImplementation(async (command: string) => {
      if (command !== "write_attachment") return [];
      await new Promise<void>((resolve) => {
        release = resolve;
      });
      return "/tmp/pasted-text.txt";
    });
    const textarea = draw({ initialDraft: "Old", draftResetToken: 0 });
    paste(textarea, "x".repeat(2_001));
    await settleUntil(() => release !== undefined);

    draw({ initialDraft: "", draftResetToken: 1 });
    release?.();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 10));
    });

    expect(
      container.querySelector('[aria-label="Remove pasted-text.txt"]'),
    ).toBeNull();
    expect(container.querySelector("textarea")!.value).toBe("");
  });
});
