// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));
vi.mock("../../source-control/hooks/useProjectBranches", () => ({
  useProjectBranchesState: () => ({
    branches: {
      current: "mc/greeting",
      detached: false,
      branches: [
        { name: "mc/greeting", remote: null, current: true },
        { name: "main", remote: null, current: false },
      ],
    },
    settled: true,
  }),
}));

import { Composer, ComposerAction } from "./Composer";
import type { ComposerTurnOptions, Attachment } from "../model/session";
import type { UserQuestionPrompt } from "../model/userQuestion";

/**
 * The assertions below pin English copy (button labels, placeholders), and the
 * interface language follows the machine's locale, so pin it for the file:
 * otherwise the expectations depend on where the tests run.
 */
beforeEach(() => {
  localStorage.setItem("monocode.language", "en");
});

afterEach(() => {
  localStorage.removeItem("monocode.language");
});

function renderAction(busy: boolean, hasValue: boolean) {
  return renderToStaticMarkup(
    createElement(ComposerAction, {
      busy,
      hasValue,
      onSend: vi.fn(),
      onStop: vi.fn(),
    }),
  );
}

describe("ComposerAction", () => {
  it("replaces Stop with Send when typing during a running turn", () => {
    const empty = renderAction(true, false);
    expect(empty).toContain('aria-label="Stop"');
    expect(empty).not.toContain('aria-label="Send"');

    const typed = renderAction(true, true);
    expect(typed).toContain('aria-label="Send"');
    expect(typed).toContain("composer-send");
    expect(typed).toContain("primary-action");
    expect(typed).not.toContain('aria-label="Stop"');
  });
});

describe("Composer question focus", () => {
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

  const question: UserQuestionPrompt = {
    requestId: 1,
    questions: [
      {
        id: "q1",
        prompt: "Pick one",
        multiSelect: false,
        allowCustom: false,
        options: [{ id: "a", label: "Option A" }],
      },
    ],
  };

  async function renderComposer(
    currentQuestion: UserQuestionPrompt | undefined,
    onQuestionReply: (requestId: number, reply: unknown) => void,
    busy = false,
    focusToken = 0,
    initialDraft?: string,
    onBtwCommand?: (text: string) => boolean | void,
    onSubmit: (text: string, attachments: Attachment[]) => void = () => {},
  ) {
    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: true,
          focusToken,
          harness: "claude",
          model: "claude-sonnet",
          runtimeMode: "supervised",
          executionCwd: "/repo",
          initialDraft,
          hideProjectPicker: true,
          hideBranchPicker: true,
          onFocus: () => {},
          onCwdChange: () => {},
          onModelChange: () => {},
          onRuntimeModeChange: () => {},
          onSubmit,
          onBtwCommand,
          question: currentQuestion,
          onQuestionReply,
          busy,
        }),
      ),
    );
  }

  it.each([
    ["/btw", ""],
    ["/btw some text here...", "some text here..."],
  ])("routes %s to BTW instead of the main submit", async (draft, text) => {
    const onBtwCommand = vi.fn(() => true);
    const onSubmit = vi.fn();
    await renderComposer(
      undefined,
      vi.fn(),
      false,
      0,
      draft,
      onBtwCommand,
      onSubmit,
    );
    const textarea = container.querySelector("textarea")!;
    await act(async () =>
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Enter",
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(onBtwCommand).toHaveBeenCalledWith(text);
    expect(onSubmit).not.toHaveBeenCalled();
    expect(textarea.value).toBe("");
  });

  it("keeps the draft when onBtwCommand rejects the command", async () => {
    const onBtwCommand = vi.fn(() => false);
    const onSubmit = vi.fn();
    await renderComposer(
      undefined,
      vi.fn(),
      false,
      0,
      "/btw",
      onBtwCommand,
      onSubmit,
    );
    const textarea = container.querySelector("textarea")!;
    await act(async () =>
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Enter",
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(onBtwCommand).toHaveBeenCalledWith("");
    expect(onSubmit).not.toHaveBeenCalled();
    expect(textarea.value).toBe("/btw");
  });

  it("clears the draft when the reset token advances", async () => {
    const onDraftChange = vi.fn();
    const props = {
      focused: true,
      harness: "claude" as const,
      model: "claude-sonnet",
      runtimeMode: "supervised" as const,
      executionCwd: "/repo",
      hideProjectPicker: true,
      hideBranchPicker: true,
      initialDraft: "something here...",
      draftResetToken: 1,
      onDraftChange,
      onFocus: vi.fn(),
      onCwdChange: vi.fn(),
      onModelChange: vi.fn(),
      onRuntimeModeChange: vi.fn(),
      onSubmit: vi.fn(),
    };
    await act(async () => root.render(createElement(Composer, props)));
    expect(container.querySelector("textarea")?.value).toBe(
      "something here...",
    );

    await act(async () =>
      root.render(createElement(Composer, { ...props, draftResetToken: 2 })),
    );
    expect(container.querySelector("textarea")?.value).toBe("");
    expect(onDraftChange).toHaveBeenLastCalledWith("");
  });

  it("keeps drafts and blocks sending until a working copy is selected", async () => {
    const onSubmit = vi.fn();
    const props = {
      harness: "claude" as const,
      model: "claude-sonnet",
      runtimeMode: "supervised" as const,
      executionCwd: "/deleted-worktree",
      hideProjectPicker: true,
      hideBranchPicker: true,
      initialDraft: "Continue this feature",
      onFocus: vi.fn(),
      onCwdChange: vi.fn(),
      onModelChange: vi.fn(),
      onRuntimeModeChange: vi.fn(),
      onSubmit,
    };
    await act(async () =>
      root.render(createElement(Composer, { ...props, worktreeRemoved: true })),
    );
    const textarea = container.querySelector("textarea")!;
    const send = container.querySelector<HTMLButtonElement>(
      '[aria-label="Send"]',
    )!;
    expect(send.disabled).toBe(true);
    expect(textarea.placeholder).toContain("Select a branch or worktree");
    await act(async () =>
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Enter",
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
    expect(textarea.value).toBe("Continue this feature");
    await act(async () =>
      root.render(
        createElement(Composer, { ...props, worktreeRemoved: false }),
      ),
    );
    expect(send.disabled).toBe(false);
    await act(async () => send.click());
    expect(onSubmit).toHaveBeenCalledWith("Continue this feature", [], {
      intent: "default",
    });
  });

  it("places the caret at the end of an initial draft", async () => {
    const initialDraft = "Comment on src/App.tsx:42\n\n";
    await renderComposer(undefined, vi.fn(), false, 0, initialDraft);

    const textarea = container.querySelector("textarea")!;
    expect(textarea.value).toBe(initialDraft);
    expect(textarea.selectionStart).toBe(initialDraft.length);
    expect(textarea.selectionEnd).toBe(initialDraft.length);
  });

  it("offers /operator in the slash picker and submits it as a local command", async () => {
    const onSubmit = vi.fn(() => true);
    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: true,
          harness: "claude",
          model: "claude-sonnet",
          runtimeMode: "supervised",
          executionCwd: "/repo",
          initialDraft: "/operator",
          hideProjectPicker: true,
          hideBranchPicker: true,
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit,
        }),
      ),
    );
    const textarea = container.querySelector("textarea")!;
    await act(async () =>
      textarea.dispatchEvent(new Event("input", { bubbles: true })),
    );
    const command = Array.from(
      container.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ).find((button) => button.textContent?.includes("/operator"));
    expect(command).toBeDefined();
    await act(async () => command!.click());
    expect(textarea.value).toBe("/operator ");
    await act(async () => {
      textarea.value += "list my notes";
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Send"]')!
        .click(),
    );
    expect(onSubmit).toHaveBeenCalledWith("/operator list my notes", [], {
      intent: "default",
    });
  });

  it("offers Operator above Orchestrator and sends the /operator command", async () => {
    const onSubmit = vi.fn().mockReturnValueOnce(false).mockReturnValue(true);
    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: true,
          harness: "claude",
          model: "claude-sonnet",
          runtimeMode: "supervised",
          executionCwd: "/repo",
          initialDraft: "List my notes",
          hideProjectPicker: true,
          hideBranchPicker: true,
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit,
        }),
      ),
    );
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Add files or choose a mode"]',
        )!
        .click(),
    );
    const options = Array.from(
      document.querySelectorAll<HTMLButtonElement>(
        "[data-composer-plus] button",
      ),
    );
    const operator = options.find((button) =>
      button.textContent?.includes("Operator"),
    )!;
    expect(operator).toBeDefined();
    expect(options.indexOf(operator)).toBeLessThan(
      options.findIndex((button) =>
        button.textContent?.includes("Orchestrator"),
      ),
    );
    await act(async () => operator.click());
    const textarea = container.querySelector("textarea")!;
    expect(textarea.value).toBe("List my notes");
    expect(
      container.querySelector('[aria-label="Turn off Operator"]'),
    ).not.toBeNull();

    const send = container.querySelector<HTMLButtonElement>(
      '[aria-label="Send"]',
    )!;
    await act(async () => send.click());
    expect(onSubmit).toHaveBeenLastCalledWith("/operator List my notes", [], {
      intent: "default",
    });
    expect(textarea.value).toBe("List my notes");
    expect(
      container.querySelector('[aria-label="Turn off Operator"]'),
    ).not.toBeNull();

    await act(async () => {
      textarea.value = "/operator List my notes";
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
      send.click();
    });
    expect(onSubmit).toHaveBeenCalledTimes(2);
    expect(onSubmit).toHaveBeenLastCalledWith("/operator List my notes", [], {
      intent: "default",
    });
    expect(textarea.value).toBe("");
    expect(
      container.querySelector('[aria-label="Turn off Operator"]'),
    ).toBeNull();
  });

  it("clears the parent draft before submit so a remounting composer stays empty", async () => {
    let parentDraft = "Ship the empty-state fix";
    const onDraftChange = vi.fn((text: string) => {
      parentDraft = text;
    });
    const baseProps = {
      focused: true,
      harness: "claude" as const,
      model: "claude-sonnet",
      runtimeMode: "supervised" as const,
      executionCwd: "/repo",
      hideProjectPicker: true,
      hideBranchPicker: true,
      initialDraft: parentDraft,
      onDraftChange,
      onFocus: vi.fn(),
      onCwdChange: vi.fn(),
      onModelChange: vi.fn(),
      onRuntimeModeChange: vi.fn(),
    };
    const onSubmit = vi.fn(() => {
      act(() =>
        root.render(
          createElement(Composer, {
            ...baseProps,
            key: "docked",
            initialDraft: parentDraft,
            onSubmit,
          }),
        ),
      );
      return true;
    });
    await act(async () =>
      root.render(
        createElement(Composer, { ...baseProps, key: "empty", onSubmit }),
      ),
    );

    const textarea = container.querySelector("textarea")!;
    expect(textarea.value).toBe("Ship the empty-state fix");
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Send"]')!
        .click(),
    );

    expect(onSubmit).toHaveBeenCalledWith("Ship the empty-state fix", [], {
      intent: "default",
    });
    expect(onDraftChange).toHaveBeenCalledWith("");
    expect(parentDraft).toBe("");
    expect(textarea.value).toBe("");
  });

  it("restores the draft when submit is rejected", async () => {
    let parentDraft = "Blocked while orchestration is paused";
    const onDraftChange = vi.fn((text: string) => {
      parentDraft = text;
    });
    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: true,
          harness: "claude",
          model: "claude-sonnet",
          runtimeMode: "supervised",
          executionCwd: "/repo",
          hideProjectPicker: true,
          hideBranchPicker: true,
          initialDraft: parentDraft,
          onDraftChange,
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit: () => false,
        }),
      ),
    );

    const textarea = container.querySelector("textarea")!;
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Send"]')!
        .click(),
    );

    expect(textarea.value).toBe("Blocked while orchestration is paused");
    expect(parentDraft).toBe("Blocked while orchestration is paused");
  });

  it("does not restore a failed resend over newer composer text", async () => {
    let recallLastTurn: (() => void) | undefined;
    let rejectResend: ComposerTurnOptions["onResendRejected"];
    const onSubmit = vi.fn(
      (_text: string, _files: Attachment[], options?: ComposerTurnOptions) => {
        rejectResend = options?.onResendRejected;
        return true;
      },
    );

    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: true,
          harness: "pi",
          model: "pi:default",
          runtimeMode: "supervised",
          executionCwd: "/repo",
          hideProjectPicker: true,
          hideBranchPicker: true,
          editLastTurnSupported: true,
          lastTurnRecall: { text: "Original prompt", attachments: [] },
          onRecallLastTurnReady: (recall) => {
            recallLastTurn = recall;
          },
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit,
        }),
      ),
    );

    await act(async () => recallLastTurn?.());
    const textarea = container.querySelector("textarea")!;
    expect(textarea.value).toBe("Original prompt");

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Send"]')!
        .click(),
    );
    await act(async () => {
      textarea.value = "New prompt";
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => rejectResend?.({ providerRewound: false }));

    expect(textarea.value).toBe("New prompt");
  });

  it("retries an already rewound prompt as a normal submission", async () => {
    let recallLastTurn: (() => void) | undefined;
    let rejectResend: ComposerTurnOptions["onResendRejected"];
    const onSubmit = vi.fn(
      (_text: string, _files: Attachment[], options?: ComposerTurnOptions) => {
        rejectResend = options?.onResendRejected;
        return true;
      },
    );

    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: true,
          harness: "pi",
          model: "pi:default",
          runtimeMode: "supervised",
          executionCwd: "/repo",
          hideProjectPicker: true,
          hideBranchPicker: true,
          editLastTurnSupported: true,
          lastTurnRecall: { text: "Edited prompt", attachments: [] },
          onRecallLastTurnReady: (recall) => {
            recallLastTurn = recall;
          },
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit,
        }),
      ),
    );

    await act(async () => recallLastTurn?.());
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Send"]')!
        .click(),
    );
    await act(async () => rejectResend?.({ providerRewound: true }));

    const textarea = container.querySelector("textarea")!;
    expect(textarea.value).toBe("Edited prompt");
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Send"]')!
        .click(),
    );

    expect(onSubmit).toHaveBeenCalledTimes(2);
    expect(onSubmit.mock.calls[1][2]).toEqual({ intent: "default" });
  });

  it("preserves attachment ownership when a resend is restored", async () => {
    const createObjectURL = vi.fn(() => "blob:owned");
    const revokeObjectURL = vi.fn();
    vi.stubGlobal("URL", { createObjectURL, revokeObjectURL });
    let recallLastTurn: (() => void) | undefined;
    let rejectResend: ComposerTurnOptions["onResendRejected"];
    const borrowed: Attachment = {
      id: "borrowed",
      name: "borrowed.png",
      mimeType: "image/png",
      kind: "image",
      size: 3,
      previewUrl: "blob:borrowed",
    };
    const onSubmit = vi.fn(
      (_text: string, _files: Attachment[], options?: ComposerTurnOptions) => {
        rejectResend = options?.onResendRejected;
        return true;
      },
    );

    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: true,
          harness: "pi",
          model: "pi:default",
          runtimeMode: "supervised",
          executionCwd: "/repo",
          hideProjectPicker: true,
          hideBranchPicker: true,
          editLastTurnSupported: true,
          lastTurnRecall: {
            text: "Edited prompt",
            attachments: [borrowed],
          },
          onRecallLastTurnReady: (recall) => {
            recallLastTurn = recall;
          },
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit,
        }),
      ),
    );

    await act(async () => recallLastTurn?.());
    const owned = new File(["new"], "owned.png", { type: "image/png" });
    const paste = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(paste, "clipboardData", {
      value: {
        getData: () => "",
        files: [owned],
        items: [
          {
            kind: "file",
            type: owned.type,
            getAsFile: () => owned,
          },
        ],
      },
    });
    await act(async () => {
      container.querySelector("textarea")!.dispatchEvent(paste);
      await new Promise((resolve) => setTimeout(resolve, 20));
    });

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Send"]')!
        .click(),
    );
    await act(async () => rejectResend?.({ providerRewound: false }));
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Remove owned.png"]')!
        .click(),
    );
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Remove borrowed.png"]')!
        .click(),
    );

    expect(createObjectURL).toHaveBeenCalledOnce();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:owned");
    expect(revokeObjectURL).not.toHaveBeenCalledWith("blob:borrowed");
  });

  it("saves a new message as a draft without submitting it", async () => {
    const onSubmit = vi.fn();
    const onSaveDraft = vi.fn();
    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: true,
          harness: "claude",
          model: "claude-sonnet",
          runtimeMode: "supervised",
          executionCwd: "/repo",
          hideProjectPicker: true,
          hideBranchPicker: true,
          canSaveDraft: true,
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit,
          onSaveDraft,
        }),
      ),
    );

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Add files or choose a mode"]',
        )!
        .click(),
    );
    const draftMode = [
      ...document.querySelectorAll<HTMLButtonElement>("button"),
    ].find((button) => button.textContent?.includes("Save this message"));
    expect(draftMode).toBeDefined();
    await act(async () => draftMode!.click());

    const textarea = container.querySelector("textarea")!;
    await act(async () => {
      textarea.value = "Explore a quieter empty state";
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const save = container.querySelector<HTMLButtonElement>(
      '[aria-label="Save draft"]',
    );
    expect(save?.disabled).toBe(false);
    await act(async () => save!.click());

    expect(onSaveDraft).toHaveBeenCalledWith(
      "Explore a quieter empty state",
      [],
    );
    expect(onSubmit).not.toHaveBeenCalled();
    expect(textarea.value).toBe("");
  });

  it("locks a started session to its worktree while keeping its branch editable", async () => {
    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: true,
          harness: "claude",
          model: "claude-sonnet",
          runtimeMode: "supervised",
          cwd: "/repo",
          executionCwd: "/repo-worktrees/mc-greeting",
          hideProjectPicker: true,
          onFocus: vi.fn(),
          onCwdChange: vi.fn(),
          onWorktreeChange: vi.fn(async () => {}),
          onModelChange: vi.fn(),
          onRuntimeModeChange: vi.fn(),
          onSubmit: vi.fn(),
        }),
      ),
    );

    const workspace = container.querySelector(
      '[aria-label="Workspace Worktree"]',
    );
    expect(workspace?.tagName).toBe("DIV");
    expect(
      container.querySelector('[aria-label="Choose working copy"]'),
    ).toBeNull();
    expect(
      container.querySelector('[aria-label="Branch mc/greeting"]'),
    ).not.toBeNull();
  });

  it("toggles a draft between the current checkout and a new worktree", async () => {
    const onWorkspaceModeChange = vi.fn();
    const onWorktreeBaseChange = vi.fn();
    const props = {
      focused: true,
      harness: "claude" as const,
      model: "claude-sonnet",
      runtimeMode: "supervised" as const,
      cwd: "/repo",
      executionCwd: "/repo",
      branch: "main",
      hideProjectPicker: true,
      draftWorkspace: true,
      onFocus: vi.fn(),
      onCwdChange: vi.fn(),
      onBranchChange: vi.fn(async () => {}),
      onWorktreeChange: vi.fn(async () => {}),
      onWorkspaceModeChange,
      onWorktreeBaseChange,
      onModelChange: vi.fn(),
      onRuntimeModeChange: vi.fn(),
      onSubmit: vi.fn(),
    };
    await act(async () =>
      root.render(
        createElement(Composer, { ...props, workspaceMode: "current" }),
      ),
    );
    const textarea = container.querySelector("textarea")!;
    const workspace = container.querySelector<HTMLButtonElement>(
      '[aria-label="Workspace Current checkout"]',
    )!;
    expect(
      container.querySelector('[aria-label="Branch main"]'),
    ).not.toBeNull();
    await act(async () => workspace.click());
    expect(document.body.textContent).toContain("Existing worktree…");
    expect(
      container.querySelector('[aria-label="Branch main"]'),
    ).not.toBeNull();
    await act(async () => workspace.click());

    await act(async () =>
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "g",
          ctrlKey: true,
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(onWorkspaceModeChange).toHaveBeenLastCalledWith("worktree", "main");

    await act(async () =>
      root.render(
        createElement(Composer, { ...props, workspaceMode: "worktree" }),
      ),
    );
    await act(async () =>
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "G",
          ctrlKey: true,
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(onWorkspaceModeChange).toHaveBeenLastCalledWith(
      "current",
      undefined,
    );

    await act(async () =>
      root.render(
        createElement(Composer, {
          ...props,
          branch: undefined,
          workspaceMode: "current",
        }),
      ),
    );
    await act(async () =>
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "g",
          ctrlKey: true,
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(onWorkspaceModeChange).toHaveBeenLastCalledWith(
      "worktree",
      "mc/greeting",
    );

    await act(async () =>
      root.render(
        createElement(Composer, {
          ...props,
          branch: undefined,
          workspaceMode: "worktree",
          worktreeBase: "HEAD",
        }),
      ),
    );
    expect(onWorktreeBaseChange).toHaveBeenLastCalledWith("mc/greeting");
  });

  it("returns focus to the composer textarea once a question is answered", async () => {
    const onQuestionReply = vi.fn();
    await renderComposer(question, onQuestionReply);

    await act(async () =>
      (
        container.querySelector("button[aria-pressed]") as HTMLButtonElement
      ).click(),
    );
    await act(async () =>
      (
        container.querySelector('button[type="submit"]') as HTMLButtonElement
      ).click(),
    );
    expect(onQuestionReply).toHaveBeenCalledWith(1, {
      kind: "answered",
      answers: { q1: ["a"] },
    });

    // The real app clears `question` once onQuestionReply resolves it.
    await renderComposer(undefined, onQuestionReply);

    expect(document.activeElement).toBe(container.querySelector("textarea"));
  });

  it("returns focus to the composer textarea once the agent turn finishes", async () => {
    await renderComposer(undefined, vi.fn(), true);
    const textarea = container.querySelector("textarea") as HTMLTextAreaElement;

    const decoy = document.createElement("input");
    document.body.append(decoy);
    decoy.focus();
    expect(document.activeElement).toBe(decoy);

    await renderComposer(undefined, vi.fn(), false);

    expect(document.activeElement).toBe(textarea);
    decoy.remove();
  });

  it("returns focus to the composer textarea when focusToken bumps while already focused", async () => {
    await renderComposer(undefined, vi.fn());
    const textarea = container.querySelector("textarea") as HTMLTextAreaElement;

    const decoy = document.createElement("input");
    document.body.append(decoy);
    decoy.focus();
    expect(document.activeElement).toBe(decoy);

    // `focused` never changes value here (stays true throughout) — mirrors a
    // real window blur/refocus, where React's composerFocused state doesn't
    // change even though the OS took DOM focus away and back.
    await renderComposer(undefined, vi.fn(), false, 1);

    expect(document.activeElement).toBe(textarea);
    decoy.remove();
  });

  it("does not steal focus from a control inside a different composer", async () => {
    await renderComposer(undefined, vi.fn(), true);

    // Simulates focus reaching another mounted Composer's control via
    // keyboard Tab navigation, which never fires the onMouseDown-based
    // onFocus that would normally update which pane is "focused".
    const otherComposer = document.createElement("div");
    otherComposer.setAttribute("data-composer", "");
    const otherInput = document.createElement("textarea");
    otherComposer.append(otherInput);
    document.body.append(otherComposer);
    otherInput.focus();
    expect(document.activeElement).toBe(otherInput);

    await renderComposer(undefined, vi.fn(), false);

    expect(document.activeElement).toBe(otherInput);
    otherComposer.remove();
  });

  it("takes focus from a composer in a hidden session", async () => {
    const hiddenSession = document.createElement("div");
    hiddenSession.setAttribute("aria-hidden", "true");
    const hiddenComposer = document.createElement("div");
    hiddenComposer.setAttribute("data-composer", "");
    const hiddenInput = document.createElement("textarea");
    hiddenComposer.append(hiddenInput);
    hiddenSession.append(hiddenComposer);
    document.body.append(hiddenSession);
    hiddenInput.focus();
    expect(document.activeElement).toBe(hiddenInput);

    await renderComposer(undefined, vi.fn());

    expect(document.activeElement).toBe(container.querySelector("textarea"));
    hiddenSession.remove();
  });

  it("does not steal focus from a picker portaled outside the composer", async () => {
    await renderComposer(undefined, vi.fn(), true);

    // Popover.tsx portals picker content directly into document.body, so it
    // never sits under this composer's own [data-composer] subtree.
    const portaledPicker = document.createElement("div");
    portaledPicker.setAttribute("data-model-picker", "");
    const searchInput = document.createElement("input");
    portaledPicker.append(searchInput);
    document.body.append(portaledPicker);
    searchInput.focus();
    expect(document.activeElement).toBe(searchInput);

    await renderComposer(undefined, vi.fn(), false);

    expect(document.activeElement).toBe(searchInput);
    portaledPicker.remove();
  });
});
