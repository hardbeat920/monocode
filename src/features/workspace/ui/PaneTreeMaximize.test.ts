// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Session } from "../../sessions/model/session";
import type { EditorPane, LayoutNode } from "../model/layout";
import { PaneTree } from "./PaneTree";

const killPty = vi.hoisted(() => vi.fn());
const sessionVisible = vi.hoisted(() => new Map<string, boolean>());

// Stand-ins that own the state a real unmount would lose: an uncontrolled
// editor buffer, a terminal whose cleanup kills its PTY (as TerminalView
// does), and composer attachments held in component state.
vi.mock("../../files/ui/FilePane", async () => {
  const { createElement, useEffect } = await import("react");
  return {
    FilePane: ({ pane }: { pane: EditorPane }) => {
      useEffect(() => () => killPty(pane.id), [pane.id]);
      return createElement("textarea", { "data-editor": pane.id });
    },
  };
});

vi.mock("../../sessions/ui/SessionPane", async () => {
  const { createElement, useState } = await import("react");
  return {
    SessionPane: ({
      session,
      visible,
    }: {
      session: Session;
      visible: boolean;
    }) => {
      sessionVisible.set(session.id, visible);
      const [attachments, setAttachments] = useState(0);
      return createElement(
        "button",
        {
          "data-composer": session.id,
          "data-attachments": attachments,
          onClick: () => setAttachments((count) => count + 1),
        },
        "attach",
      );
    },
  };
});

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
  killPty.mockClear();
  sessionVisible.clear();
  vi.unstubAllGlobals();
});

const layout: LayoutNode = {
  type: "split",
  id: "split",
  dir: "right",
  children: [
    { type: "leaf", id: "chat" },
    { type: "leaf", id: "editor" },
    { type: "leaf", id: "terminal" },
  ],
  sizes: [1 / 3, 1 / 3, 1 / 3],
};

function render(maximizedId?: string) {
  const noop = vi.fn();
  const props: ComponentProps<typeof PaneTree> = {
    visible: true,
    layout,
    sessions: [{ id: "chat" } as Session],
    editorPanes: [
      { id: "editor", files: [], activeFileId: "" },
      { id: "terminal", files: [], activeFileId: "" },
    ],
    dirtyFileIds: new Set(),
    fileErrorCounts: new Map(),
    focusedId: maximizedId ?? "chat",
    maximizedId,
    composerFocused: false,
    recents: [],
    onFocus: noop,
    onClose: noop,
    onSelectFile: noop,
    onCloseFile: noop,
    onCloseOtherFiles: noop,
    onReorderFiles: noop,
    onFileDirtyChange: noop,
    onFileErrorCountChange: noop,
    onRatio: noop,
    onCwdChange: noop,
    onBranchChange: noop,
    onModelChange: noop,
    onModelSettingsChange: noop,
    onRuntimeModeChange: noop,
    onSubmit: noop,
    onStop: noop,
    onCompactContext: noop,
    onPlaceSessionInFolder: noop,
    onDeleteQueuedMessage: noop,
    onEditQueuedMessage: noop,
    onQueuedMessageEditingChange: noop,
    onSteerQueuedMessage: noop,
    onResumeQueue: noop,
    onApproval: noop,
    onQuestionReply: noop,
    onOpenFile: noop,
    onOpenDiff: noop,
    onOpenPlan: noop,
    onUpdatePlan: noop,
    onBuildPlan: noop,
    onMovePane: noop,
    onDetachPane: noop,
    onNewTerminal: noop,
  };
  act(() => root.render(createElement(PaneTree, props)));
}

const pane = (id: string) =>
  container.querySelector<HTMLElement>(`[data-pane-id="${id}"]`)!;
const editor = () =>
  container.querySelector<HTMLTextAreaElement>('[data-editor="editor"]')!;
const composer = () =>
  container.querySelector<HTMLButtonElement>('[data-composer="chat"]')!;

describe("maximize keeps sibling panes mounted", () => {
  it("preserves unsaved edits, attachments, and the running terminal across maximize/restore", () => {
    render();
    editor().value = "unsaved edit";
    act(() => composer().click());
    const editorNode = editor();

    render("editor");
    expect(pane("editor").classList.contains("hidden")).toBe(false);
    for (const id of ["chat", "terminal"]) {
      expect(pane(id).classList.contains("hidden")).toBe(true);
      expect(pane(id).getAttribute("aria-hidden")).toBe("true");
      expect(pane(id).hasAttribute("inert")).toBe(true);
    }
    expect(sessionVisible.get("chat")).toBe(false);
    expect(killPty).not.toHaveBeenCalled();

    render();
    expect(editor()).toBe(editorNode);
    expect(editor().value).toBe("unsaved edit");
    expect(composer().dataset.attachments).toBe("1");
    expect(sessionVisible.get("chat")).toBe(true);
    for (const id of ["chat", "editor", "terminal"]) {
      expect(pane(id).classList.contains("hidden")).toBe(false);
      expect(pane(id).hasAttribute("aria-hidden")).toBe(false);
    }
    expect(killPty).not.toHaveBeenCalled();
  });

  it("fills the tree with the maximized pane", () => {
    render("terminal");
    const style = pane("terminal").style;
    expect([style.left, style.top, style.width, style.height]).toEqual([
      "0%",
      "0%",
      "100%",
      "100%",
    ]);
  });
});
