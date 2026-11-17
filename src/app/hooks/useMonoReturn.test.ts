// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { MonoScroll } from "../../features/monos/model/monoScroll";
import type { OverviewSearch } from "../../features/monos/ui/MonoOverview";
import { DEFAULT_SESSION_SIDEBAR_FILTERS } from "../../features/sessions/model/sessionFilters";
import {
  newEditorWorkspaceTab,
  newFileTab,
  type WorkspaceTab,
} from "../../features/workspace/model/layout";
import type { MonoOverviewView } from "../model/monoReturn";
import { useMonoReturn } from "./useMonoReturn";

const overview: MonoOverviewView = {
  shownSession: "launched",
  shownSessionFocused: true,
  document: "doc-1",
  activity: { sessionId: "chat", turnId: "t1", blocks: [] },
};
const scroll: MonoScroll = {
  overview: { tab: "documents", top: 240 },
  document: { id: "doc-1", top: 90 },
};
const search: OverviewSearch = {
  query: "deploy",
  filters: DEFAULT_SESSION_SIDEBAR_FILTERS,
};

let container: HTMLDivElement;
let root: Root;
let api: ReturnType<typeof useMonoReturn>;
let state: {
  tabs: WorkspaceTab[];
  activeTabId: string;
  covered: boolean;
  shown: string | null;
  request: number;
  /** Sessions that exist, parked or not. */
  sessions: Set<string>;
  view: MonoOverviewView;
  /** What the overview scrolls and searches, readable while it is mounted. */
  mounted: boolean;
};
/** The restore each Mono open was handed, as `onOpenMono` would take it. */
let opened: {
  monoId: string;
  restoreFor: ReturnType<typeof api.monoOpening>;
}[];
const openMono = vi.fn();

function Harness() {
  api = useMonoReturn({
    tabs: state.tabs,
    activeTabId: state.activeTabId,
    covered: state.covered,
    shownMonoSession: () => state.shown,
    viewRequest: () => state.request,
    monoOfSession: (id) => (id === "chat" ? "mono-1" : undefined),
    hasSession: async (id) => state.sessions.has(id),
    canOpenMono: (id) => id === "mono-1",
    openMono,
    view: state.view,
    readOverview: () => (state.mounted ? { scroll, search } : {}),
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
/** What closing a file does: start, move on to a neighbour, finish. */
function closeFile(
  tabId: string,
  paths: string[],
  moveOn: () => void = () => {},
) {
  api.closeStarting(tabId, paths);
  moveOn();
  api.closeFinished();
}
const restored = (monoId = "mono-1", sessionId = "chat") =>
  opened.find((entry) => entry.monoId === monoId)?.restoreFor(sessionId);

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  opened = [];
  openMono.mockReset();
  // The real open takes the return over as it starts.
  openMono.mockImplementation((monoId: string) =>
    opened.push({ monoId, restoreFor: api.monoOpening(monoId) }),
  );
  state = {
    tabs: [],
    activeTabId: "",
    covered: false,
    shown: null,
    request: 0,
    sessions: new Set(["chat", "launched"]),
    view: structuredClone(overview),
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

it("returns to the Mono with its overview, scroll and search", async () => {
  const tab = openFromMono();
  closeFile(tab.id, ["/p/notes.md"]);
  expect(openMono).toHaveBeenCalledExactlyOnceWith("mono-1");
  expect(await restored()).toEqual({ ...overview, scroll, search });
});

it("only returns once, and only for that file in that tab", () => {
  const tab = openFromMono();
  closeFile(tab.id, ["/p/other.md"]);
  closeFile("another-tab", ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
  closeFile(tab.id, ["/p/notes.md"]);
  closeFile(tab.id, ["/p/notes.md"]);
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
  closeFile(tab.id, ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("ends when focus leaves the file's pane, or its tab", () => {
  const tab = openFromMono();
  state.tabs = [{ ...tab, focusedId: "a-chat-pane" }];
  render();
  closeFile(tab.id, ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();

  const second = openFromMono("/p/second.md");
  state.activeTabId = "elsewhere";
  render();
  state.activeTabId = second.id;
  render();
  closeFile(second.id, ["/p/second.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("ends when search, settings or another view covered the file", () => {
  const tab = openFromMono();
  state.covered = true; // Search opened over the file
  render();
  state.covered = false; // and closed again, back on the file
  render();
  closeFile(tab.id, ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("ends for a file opened behind the active tab, even if shown later", () => {
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
  state.activeTabId = background.id; // looked at later
  render();
  closeFile(background.id, ["/p/background.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("is not ended by a pass from before the open was recorded", () => {
  state.shown = "chat";
  const opening = api.beginFileOpen(() => (state.shown = null));
  opening.resolved();
  opening.opened("tab-new", "/p/notes.md");
  const tab = { ...fileTab("/p/notes.md"), id: "tab-new" };
  state.tabs = [tab];
  state.activeTabId = tab.id;
  render();
  closeFile(tab.id, ["/p/notes.md"]);
  expect(openMono).toHaveBeenCalledOnce();
});

it("does not return for a file never seen in view", () => {
  state.shown = "chat";
  const opening = api.beginFileOpen(() => (state.shown = null));
  opening.resolved();
  opening.opened("tab-bg", "/p/notes.md");
  closeFile("tab-bg", ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("starts fresh when a file opens again, from a Mono or not", () => {
  const tab = openFromMono();
  api.beginFileOpen(() => {}).opened(tab.id, "/p/notes.md");
  closeFile(tab.id, ["/p/notes.md"]);
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
  closeFile("tab-1", ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("leaves the close alone when a Mono shows", () => {
  const tab = openFromMono();
  state.shown = "chat";
  closeFile(tab.id, ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("ends when any Mono opens by the user's own doing", () => {
  const tab = openFromMono();
  api.monoOpening("mono-2");
  closeFile(tab.id, ["/p/notes.md"]);
  expect(openMono).not.toHaveBeenCalled();
});

it("keeps the return when closing selects this Mono as the neighbour", async () => {
  const tab = openFromMono();
  // The inset layout's close selects the next title tab: this Mono's own.
  closeFile(tab.id, ["/p/notes.md"], () =>
    opened.push({ monoId: "mono-1", restoreFor: api.monoOpening("mono-1") }),
  );
  expect(openMono).not.toHaveBeenCalled(); // that open is the return
  expect(await restored()).toEqual({ ...overview, scroll, search });
});

it("still returns when closing selects another Mono as the neighbour", async () => {
  const tab = openFromMono();
  closeFile(tab.id, ["/p/notes.md"], () =>
    opened.push({ monoId: "mono-2", restoreFor: api.monoOpening("mono-2") }),
  );
  expect(openMono).toHaveBeenCalledExactlyOnceWith("mono-1");
  expect(await restored("mono-2")).toBeNull();
  expect(await restored("mono-1")).toEqual({ ...overview, scroll, search });
});

it("restores a parked session, and drops one that is gone", async () => {
  const tab = openFromMono();
  closeFile(tab.id, ["/p/notes.md"]);
  expect(await restored()).toMatchObject({ shownSession: "launched" });

  const gone = openFromMono("/p/gone.md");
  state.sessions.delete("launched");
  closeFile(gone.id, ["/p/gone.md"]);
  expect(
    await opened.find((entry, i) => i === 1)?.restoreFor("chat"),
  ).toMatchObject({
    shownSession: null,
    shownSessionFocused: false,
    // The Mono's own chat is still there.
    activity: overview.activity,
  });
});

it("drops a deleted document, with its scroll, even one deleted mid-open", async () => {
  const tab = openFromMono();
  closeFile(tab.id, ["/p/notes.md"]);
  const restoreFor = opened[0].restoreFor;
  const pending = restoreFor("chat"); // loading the shown session
  api.documentDeleted("doc-1"); // deleted while it loads
  const view = await pending;
  expect(view?.document).toBeNull();
  expect(view?.scroll?.document).toBeUndefined();
  expect(view?.scroll?.overview).toEqual(scroll.overview);
  expect(view?.shownSession).toBe("launched");
});

it("restores only for the session it came from", async () => {
  const tab = openFromMono();
  closeFile(tab.id, ["/p/notes.md"]);
  expect(await restored("mono-1", "other-chat")).toBeNull();
});
