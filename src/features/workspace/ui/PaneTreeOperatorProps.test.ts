// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import type { SessionPaneProps } from "../../sessions/ui/SessionPane";
import { PaneTree } from "./PaneTree";

// PaneTree forwards pane props one by one, so a prop that App passes can be
// dropped on the main tab path while the inbox path, which spreads, keeps it.
const received: SessionPaneProps[] = [];
vi.mock("../../sessions/ui/SessionPane", () => ({
  SessionPane: (props: SessionPaneProps) => {
    received.push(props);
    return null;
  },
}));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  received.length = 0;
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("operator props through the pane tree", () => {
  it("hands the session pane the open, linked-agents and peer lookups", () => {
    const session = newSession("claude", "/tmp/project");
    const noop = vi.fn();
    const onOpenSession = vi.fn();
    const linkedAgentsFor = vi.fn(() => []);
    const sessionFor = vi.fn(() => undefined);
    const props: ComponentProps<typeof PaneTree> = {
      visible: true,
      layout: { type: "leaf", id: session.id },
      sessions: [session],
      editorPanes: [],
      dirtyFileIds: new Set(),
      fileErrorCounts: new Map(),
      focusedId: session.id,
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
      onOpenSession,
      linkedAgentsFor,
      sessionFor,
      onOpenDiff: noop,
      onOpenPlan: noop,
      onUpdatePlan: noop,
      onBuildPlan: noop,
      onMovePane: noop,
      onDetachPane: noop,
      onNewTerminal: noop,
    };
    act(() => root.render(createElement(PaneTree, props)));

    const pane = received.find((item) => item.session.id === session.id);
    expect(pane).toBeDefined();
    expect(pane?.onOpenSession).toBe(onOpenSession);
    expect(pane?.linkedAgentsFor).toBe(linkedAgentsFor);
    expect(pane?.sessionFor).toBe(sessionFor);
  });
});
