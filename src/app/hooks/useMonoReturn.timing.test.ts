// @vitest-environment happy-dom
import { createElement, startTransition, useState } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  newEditorWorkspaceTab,
  newFileTab,
  type WorkspaceTab,
} from "../../features/workspace/model/layout";
import { useMonoReturn } from "./useMonoReturn";

// Real scheduling, without `act`: when effects run relative to the commit is
// what decides whether a fresh return is ever seen in view.
type Nav = { tabs: WorkspaceTab[]; active: string };
const openMono = vi.fn();
let shown: string | null;
let root: Root;
let api: ReturnType<typeof useMonoReturn>;
let navigate: (next: Nav) => void;
const settle = () => new Promise((resolve) => setTimeout(resolve, 20));
const tab = (path: string) => newEditorWorkspaceTab(newFileTab(path, "/p"));

function Harness({ first }: { first: Nav }) {
  const [nav, setNav] = useState(first);
  navigate = setNav;
  api = useMonoReturn({
    tabs: nav.tabs,
    activeTabId: nav.active,
    shownMonoSession: () => shown,
    viewRequest: () => 0,
    monoOfSession: (id) => (id === "chat" ? "mono-1" : undefined),
    covered: false,
    hasSession: async () => true,
    canOpenMono: () => true,
    openMono,
    readOverview: () => ({}),
    view: {
      shownSession: null,
      shownSessionFocused: false,
      document: null,
      activity: null,
    },
  });
  return null;
}

/** Opens `path` into `target` the way `onOpenFile` does, then closes it. */
async function openThenClose(
  mode: "default" | "flushSync" | "transition",
  start: Nav,
  target: WorkspaceTab,
  next: Nav,
) {
  root.render(createElement(Harness, { first: start }));
  await settle();
  shown = "chat";
  const opening = api.beginFileOpen(() => (shown = null));
  opening.resolved();
  const apply = () => navigate(next);
  if (mode === "flushSync") flushSync(apply);
  else if (mode === "transition") startTransition(apply);
  else apply();
  opening.opened(target.id, target.editorPanes[0].files[0].path);
  await settle();
  api.closeStarting(target.id, [target.editorPanes[0].files[0].path]);
  api.closeFinished();
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", false);
  openMono.mockReset();
  shown = null;
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  root.unmount();
  vi.unstubAllGlobals();
});

for (const mode of ["default", "flushSync", "transition"] as const) {
  it(`returns for a file opened as a new tab (${mode})`, async () => {
    const current = tab("/p/current.md");
    const opened = tab("/p/opened.md");
    await openThenClose(mode, { tabs: [current], active: current.id }, opened, {
      tabs: [current, opened],
      active: opened.id,
    });
    expect(openMono).toHaveBeenCalledOnce();
  });
}

it("returns when the open landed in a commit of its own, in the active tab", async () => {
  // A preview replaced in the tab already active: no tab change follows the
  // flushSync commit, whose effects ran before the return was recorded.
  const before = tab("/p/preview.md");
  const after = {
    ...before,
    editorPanes: [
      {
        ...before.editorPanes[0],
        files: [newFileTab("/p/opened.md", "/p")],
      },
    ],
  };
  after.editorPanes[0].activeFileId = after.editorPanes[0].files[0].id;
  await openThenClose(
    "flushSync",
    { tabs: [before], active: before.id },
    after,
    { tabs: [after], active: before.id },
  );
  expect(openMono).toHaveBeenCalledOnce();
});

it("does not return for a file opened behind the active tab", async () => {
  const current = tab("/p/current.md");
  const background = tab("/p/background.md");
  await openThenClose(
    "default",
    { tabs: [current], active: current.id },
    background,
    { tabs: [current, background], active: current.id },
  );
  expect(openMono).not.toHaveBeenCalled();
});
