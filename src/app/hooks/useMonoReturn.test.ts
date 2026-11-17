// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  newEditorWorkspaceTab,
  newFileTab,
  type WorkspaceTab,
} from "../../features/workspace/model/layout";
import type { MonoScroll } from "../../features/monos/model/monoScroll";
import type { MonoOverviewView } from "../model/monoReturn";
import { useMonoReturn } from "./useMonoReturn";

const overview: MonoOverviewView = {
  shownSession: "launched",
  shownSessionFocused: true,
  document: "doc-1",
  activity: { sessionId: "launched", turnId: "t1", blocks: [] },
};

let container: HTMLDivElement;
let root: Root;
let api: ReturnType<typeof useMonoReturn>;
let state: {
  tabs: WorkspaceTab[];
  activeTabId: string;
  shown: string | null;
  request: number;
  sessions: Set<string>;
  view: MonoOverviewView;
  scroll: MonoScroll | undefined;
  /** What the overview scrolled to is only readable while it is mounted. */
  mounted: boolean;
};
const openMono = vi.fn();

function Harness() {
  api = useMonoReturn({
    tabs: state.tabs,
    activeTabId: state.activeTabId,
    shownMonoSession: () => state.shown,
    viewRequest: () => state.request,
    monoOfSession: (id) => (id === "chat" ? "mono-1" : undefined),
    hasSession: (id) => state.sessions.has(id),
    canOpenMono: (id) => id === "mono-1",
    openMono,
    view: state.view,
    readScroll: () => (state.mounted ? state.scroll : undefined),
  });
  return null;
}
const render = () => act(() => root.render(createElement(Harness)));
const fileTab = (path: string) => newEditorWorkspaceTab(newFileTab(path, "/p"));

/** A file opened from the Mono "mono-1" into its own tab, then rendered. */
function openFromMono(path = "/p/notes.md") {
  const tab = fileTab(path);
  state.shown = "chat";
  const opening = api.beginFileOpen(() => {
    state.shown = null;
    state.request += 1;
    state.mounted = false;
  });
  opening.resolved();
  opening.opened(tab.id, path);
  state.tabs = [tab];
  state.activeTabId = tab.id;
  render();
  return tab;
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  openMono.mockReset();
  state = {
    tabs: [],
    activeTabId: "",
    shown: null,
    request: 0,
    sessions: new Set(["chat", "launched"]),
    view: structuredClone(overview),
    scroll: { overview: { tab: "documents", top: 240 }, document: { top: 90 } },
    mounted: true,
  };
  container = document.createElement("div");
  root = createRoot(container);
  render();
});
afterEach(() => {
  act(() => root.unmount());
  vi.unstubAllGlobals();
});

it("returns to the Mono with its overview when the file closes", () => {
  const tab = openFromMono();
  api.fileClosed(tab.id, ["/p/notes.md"]);
  expect(openMono).toHaveBeenCalledExactlyOnceWith("mono-1");
  expect(api.monoOpening()("chat")).toEqual({
    ...overview,
    scroll: state.scroll,
  });
});

it("reads the scroll before the Mono closes, not after", () => {
  const tab = openFromMono();
  api.fileClosed(tab.id, ["/p/notes.md"]);
  expect(api.monoOpening()("chat")?.scroll).toEqual(state.scroll);
});

it("only returns once, and only for that file in that tab", () => {
  const tab = openFromMono();
  api.fileClosed(tab.id, ["/p/other.md"]);
  api.fileClosed("another-tab", ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
  api.fileClosed(tab.id, ["/p/notes.md"]);
  api.fileClosed(tab.id, ["/p/notes.md"]);
  expect(openMono).toHaveBeenCalledOnce();
});

it("ends when focus moves to another file in the tab", () => {
  const tab = openFromMono();
  const other = newFileTab("/p/other.md", "/p");
  state.tabs = [
    {
      ...tab,
      editorPanes: [
        {
          ...tab.editorPanes[0],
          files: [...tab.editorPanes[0].files, other],
          activeFileId: other.id,
        },
      ],
    },
  ];
  render();
  api.fileClosed(tab.id, ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("ends when focus leaves the file's pane, or its tab", () => {
  const tab = openFromMono();
  state.tabs = [{ ...tab, focusedId: "a-chat-pane" }];
  render();
  api.fileClosed(tab.id, ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();

  const second = openFromMono("/p/second.md");
  state.activeTabId = "elsewhere";
  render();
  state.activeTabId = second.id;
  render();
  api.fileClosed(second.id, ["/p/second.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("is not ended by a render from before the file appeared", () => {
  state.shown = "chat";
  const opening = api.beginFileOpen(() => (state.shown = null));
  opening.resolved();
  opening.opened("tab-new", "/p/notes.md");
  render(); // an effect pass on the old tabs
  const tab = { ...fileTab("/p/notes.md"), id: "tab-new" };
  state.tabs = [tab];
  state.activeTabId = tab.id;
  render();
  api.fileClosed(tab.id, ["/p/notes.md"]);
  expect(openMono).toHaveBeenCalledOnce();
});

it("starts fresh when a file opens again, from a Mono or not", () => {
  const tab = openFromMono();
  api.beginFileOpen(() => {}).opened(tab.id, "/p/notes.md");
  api.fileClosed(tab.id, ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("is not kept when the user reached or left a Mono while it resolved", () => {
  state.shown = "chat";
  const opening = api.beginFileOpen(() => {
    state.shown = null;
    state.request += 1;
  });
  state.request += 1; // the Mono opened and closed again meanwhile
  opening.resolved();
  opening.opened("tab-1", "/p/notes.md");
  api.fileClosed("tab-1", ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("leaves the close alone when a Mono shows or is gone", () => {
  const tab = openFromMono();
  state.shown = "chat";
  api.fileClosed(tab.id, ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();

  const gone = openFromMono("/p/gone.md");
  api.beginFileOpen(() => {}); // not from a Mono: clears
  api.fileClosed(gone.id, ["/p/gone.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("ends when any Mono opens", () => {
  const tab = openFromMono();
  api.monoOpening();
  api.fileClosed(tab.id, ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("drops deleted sessions and documents from the restored view", () => {
  const tab = openFromMono();
  api.documentDeleted("doc-1");
  state.sessions.delete("launched");
  api.fileClosed(tab.id, ["/p/notes.md"]);
  expect(api.monoOpening()("chat")).toMatchObject({
    shownSession: null,
    shownSessionFocused: false,
    document: null,
    activity: null,
    // The deleted document's scroll goes with it.
    scroll: { overview: { tab: "documents", top: 240 } },
  });
});

it("drops a document deleted while the Mono is still opening", () => {
  const tab = openFromMono();
  api.fileClosed(tab.id, ["/p/notes.md"]);
  const restoreFor = api.monoOpening(); // the open awaits its session
  api.documentDeleted("doc-1");
  const view = restoreFor("chat");
  expect(view?.document).toBeNull();
  expect(view?.scroll?.document).toBeUndefined();
  expect(view?.shownSession).toBe("launched");
});

it("does not return for a file never seen in view", () => {
  state.shown = "chat";
  const opening = api.beginFileOpen(() => (state.shown = null));
  opening.resolved();
  opening.opened("tab-bg", "/p/notes.md");
  api.fileClosed("tab-bg", ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("forgets a file opened in the background after a pass or two", () => {
  const visible = fileTab("/p/visible.md");
  state.tabs = [visible];
  state.activeTabId = visible.id;
  render();
  state.shown = "chat";
  const opening = api.beginFileOpen(() => (state.shown = null));
  opening.resolved();
  const background = fileTab("/p/background.md");
  opening.opened(background.id, "/p/background.md");
  state.tabs = [visible, background];
  render(); // the open rendered, behind the active tab
  render();
  state.tabs = [visible, background];
  render();
  state.activeTabId = background.id; // looked at much later
  render();
  api.fileClosed(background.id, ["/p/background.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("restores only for the session it came from", () => {
  const tab = openFromMono();
  api.fileClosed(tab.id, ["/p/notes.md"]);
  expect(api.monoOpening()("other-chat")).toBeNull();
});
