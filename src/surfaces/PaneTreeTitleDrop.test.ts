// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getExternalTitleTabDrop } from "../lib/paneDrop";
import type { EditorPane, LayoutNode } from "../lib/layout";
import { PaneTree } from "./PaneTree";

vi.mock("./FilePane", async () => {
  const { createElement } = await import("react");
  return {
    FilePane: ({
      pane,
      onPaneDragStart,
    }: {
      pane: EditorPane;
      onPaneDragStart?: ComponentProps<"button">["onPointerDown"];
    }) =>
      createElement(
        "button",
        { "data-drag-pane": pane.id, onPointerDown: onPaneDragStart },
        pane.id,
      ),
  };
});

vi.mock("./SessionPane", () => ({ SessionPane: () => null }));

let container: HTMLDivElement;
let root: Root;

function pointer(
  target: EventTarget,
  type: string,
  clientX: number,
  clientY: number,
) {
  act(() =>
    target.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        button: 0,
        pointerId: 1,
        clientX,
        clientY,
      }),
    ),
  );
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
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

describe("pane to title tab dragging", () => {
  it("detaches a split pane at the title-tab insertion point", () => {
    const layout: LayoutNode = {
      type: "split",
      id: "split",
      dir: "right",
      children: [
        { type: "leaf", id: "first-pane" },
        { type: "leaf", id: "second-pane" },
      ],
      sizes: [0.5, 0.5],
    };
    const editorPanes: EditorPane[] = [
      { id: "first-pane", files: [], activeFileId: "" },
      { id: "second-pane", files: [], activeFileId: "" },
    ];
    const onDetachPane = vi.fn();
    const noop = vi.fn();
    const props: ComponentProps<typeof PaneTree> = {
      visible: true,
      layout,
      sessions: [],
      editorPanes,
      dirtyFileIds: new Set(),
      fileErrorCounts: new Map(),
      focusedId: "first-pane",
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
      onDetachPane,
      onNewTerminal: noop,
    };
    act(() => root.render(createElement(PaneTree, props)));

    const strip = document.createElement("div");
    strip.dataset.titleTabStrip = "";
    const titleTab = document.createElement("div");
    titleTab.dataset.titleTabId = "target-tab";
    titleTab.getBoundingClientRect = () => new DOMRect(100, 0, 100, 40);
    strip.append(titleTab);
    document.body.append(strip);
    vi.spyOn(document, "elementFromPoint").mockReturnValue(titleTab);

    const handle = container.querySelector<HTMLButtonElement>(
      '[data-drag-pane="second-pane"]',
    )!;
    const captured = new Set<number>();
    handle.setPointerCapture = (id) => captured.add(id);
    handle.hasPointerCapture = (id) => captured.has(id);
    handle.releasePointerCapture = (id) => captured.delete(id);

    pointer(handle, "pointerdown", 180, 100);
    pointer(window, "pointermove", 120, 20);
    expect(getExternalTitleTabDrop()).toEqual({
      fromId: "second-pane",
      targetTabId: "target-tab",
      position: "before",
    });
    pointer(window, "pointerup", 120, 20);

    expect(onDetachPane).toHaveBeenCalledExactlyOnceWith(
      "second-pane",
      "target-tab",
      "before",
    );
    expect(getExternalTitleTabDrop()).toBeNull();
    strip.remove();
  });
});
