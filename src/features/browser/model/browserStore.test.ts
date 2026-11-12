import { describe, expect, it } from "vitest";
import {
  closeBrowserTab,
  closeOtherBrowserTabs,
  forgetBrowserSessions,
  openBrowserTab,
  addBrowserTab,
  AGENT_TAB_TTL_MS,
  forgetBrowserSessionsIn,
  liveBrowserTabIds,
  loadGeneration,
  noteLoadStarted,
  settleLoad,
  MAX_LIVE_TABS,
  newBrowserTab,
  parseBrowserState,
  patchBrowserTabIn,
  pruneAgentTabsIn,
  serializeBrowserState,
  switchBrowserSessionIn,
  toggleBrowserDock,
  touchAgentTabIn,
  type BrowserState,
} from "./browserStore";

const empty: BrowserState = {
  docks: [],
  sessionId: "chat-a",
  lastSide: "right",
  agentTabs: {},
  recent: {},
};

describe("session browsers", () => {
  it("toggle opens a blank tab on the right, then hides and shows", () => {
    const opened = toggleBrowserDock(empty);
    expect(opened.docks).toHaveLength(1);
    expect(opened.docks[0]).toMatchObject({
      sessionId: "chat-a",
      side: "right",
      open: true,
    });
    expect(opened.docks[0].pane.files[0].browser?.url).toBe("about:blank");

    const hidden = toggleBrowserDock(opened);
    expect(hidden.docks[0].open).toBe(false);
    expect(toggleBrowserDock(hidden).docks[0].open).toBe(true);
  });

  it("does nothing without a focused session", () => {
    const none = { ...empty, sessionId: "" };
    expect(toggleBrowserDock(none)).toBe(none);
  });

  it("gives each session its own tabs and panel state", () => {
    const a = newBrowserTab("https://a.test/");
    const b = newBrowserTab("https://b.test/");
    let state = addBrowserTab(empty, "chat-a", a);
    state = addBrowserTab(state, "chat-b", b, { open: false });
    expect(state.docks.map((dock) => [dock.sessionId, dock.open])).toEqual([
      ["chat-a", true],
      ["chat-b", false],
    ]);
    // Switching sessions shows the other session's browser, closed.
    const switched = { ...state, sessionId: "chat-b" };
    expect(toggleBrowserDock(switched).docks[1].open).toBe(true);
  });

  it("opens a page's new window right after its opener", () => {
    const a = newBrowserTab("https://a.test/");
    const b = newBrowserTab("https://b.test/");
    const c = newBrowserTab("https://c.test/");
    let state = addBrowserTab(empty, "chat-a", a);
    state = addBrowserTab(state, "chat-a", b);
    state = addBrowserTab(state, "chat-a", c, { afterId: a.id });
    expect(state.docks[0].pane.files.map((file) => file.id)).toEqual([
      a.id,
      c.id,
      b.id,
    ]);
    expect(state.docks[0].pane.activeFileId).toBe(c.id);
  });

  it("adds background tabs without opening the panel", () => {
    const a = newBrowserTab("https://a.test/");
    const hidden = toggleBrowserDock(addBrowserTab(empty, "chat-a", a));
    expect(hidden.docks[0].open).toBe(false);
    const next = addBrowserTab(hidden, "chat-a", newBrowserTab("https://x/"), {
      open: false,
    });
    expect(next.docks[0].open).toBe(false);
  });

  it("patches page state and leaves unchanged tabs alone", () => {
    const tab = newBrowserTab("https://a.test/");
    const state = addBrowserTab(empty, "chat-a", tab);
    const loaded = patchBrowserTabIn(state, tab.id, {
      url: "https://a.test/next",
      title: "Next",
    });
    expect(loaded.docks[0].pane.files[0]).toMatchObject({
      path: "https://a.test/next",
      browser: { url: "https://a.test/next", title: "Next" },
    });
    expect(patchBrowserTabIn(loaded, tab.id, { title: "Next" })).toBe(loaded);
    expect(patchBrowserTabIn(loaded, "missing", { title: "x" })).toBe(loaded);
  });

  it("forgets the browsers of deleted sessions", () => {
    let state = addBrowserTab(empty, "chat-a", newBrowserTab("https://a/"));
    state = addBrowserTab(state, "chat-b", newBrowserTab("https://b/"));
    const next = forgetBrowserSessionsIn(state, ["chat-b", "unknown"]);
    expect(next.docks.map((dock) => dock.sessionId)).toEqual(["chat-a"]);
    expect(forgetBrowserSessionsIn(next, ["unknown"])).toBe(next);
  });
});

describe("live and suspended tabs", () => {
  it("keeps a session's pages running after switching away", () => {
    const a = newBrowserTab("https://a/");
    const b = newBrowserTab("https://b/");
    let state = addBrowserTab(empty, "chat-a", a);
    state = addBrowserTab(state, "chat-b", b, { open: false });
    state = switchBrowserSessionIn(state, "chat-b", 1_000);
    expect(liveBrowserTabIds(state)).toEqual(new Set([a.id, b.id]));
    expect(switchBrowserSessionIn(state, "chat-b", 2_000)).toBe(state);
  });

  it("unloads the least recently viewed pages past the limit", () => {
    let state = empty;
    const tabs: string[] = [];
    for (let i = 0; i < MAX_LIVE_TABS + 2; i++) {
      const tab = newBrowserTab(`https://${i}/`);
      tabs.push(tab.id);
      state = addBrowserTab(state, `chat-${i}`, tab);
      state = switchBrowserSessionIn(state, `chat-${i}`, i);
    }
    const live = liveBrowserTabIds(state);
    expect(live.size).toBe(MAX_LIVE_TABS);
    // The focused session (last) stays; the two oldest sessions unload.
    expect(live.has(tabs[tabs.length - 1])).toBe(true);
    expect(live.has(tabs[0])).toBe(false);
    expect(live.has(tabs[1])).toBe(false);
    expect(live.has(tabs[2])).toBe(true);
  });

  it("never unloads the focused session or agent tabs to meet the limit", () => {
    const a = newBrowserTab("https://a/");
    const b = newBrowserTab("https://b/");
    const c = newBrowserTab("https://c/");
    let state = addBrowserTab(empty, "chat-a", a);
    state = addBrowserTab(state, "chat-a", b);
    state = addBrowserTab(state, "chat-b", c, { open: false });
    state = touchAgentTabIn(state, c.id, 5);
    expect(liveBrowserTabIds(state, 1)).toEqual(new Set([a.id, b.id, c.id]));
  });

  it("keeps the focused session's tabs and agent tabs live", () => {
    const a = newBrowserTab("https://a/");
    const b = newBrowserTab("https://b/");
    const c = newBrowserTab("https://c/");
    let state = addBrowserTab(empty, "chat-a", a);
    state = addBrowserTab(state, "chat-b", b, { open: false });
    state = addBrowserTab(state, "chat-b", c, { open: false });
    expect([...liveBrowserTabIds(state)]).toEqual([a.id]);
    state = touchAgentTabIn(state, c.id, 1_000);
    expect(liveBrowserTabIds(state)).toEqual(new Set([a.id, c.id]));
  });

  it("suspends agent tabs after a quiet period or once closed", () => {
    const a = newBrowserTab("https://a/");
    let state = addBrowserTab(empty, "chat-b", a, { open: false });
    state = touchAgentTabIn(state, a.id, 1_000);
    expect(pruneAgentTabsIn(state, 2_000)).toBe(state);
    expect(pruneAgentTabsIn(state, 1_000 + AGENT_TAB_TTL_MS).agentTabs).toEqual(
      {},
    );
    const closed = forgetBrowserSessionsIn(state, ["chat-b"]);
    expect(pruneAgentTabsIn(closed, 2_000).agentTabs).toEqual({});
  });
});

describe("browser persistence", () => {
  it("round-trips docks without live page state", () => {
    const tab = newBrowserTab("https://a.test/");
    const state = patchBrowserTabIn(
      addBrowserTab(empty, "chat-a", tab),
      tab.id,
      { title: "A", loading: true, error: "boom" },
    );
    const restored = parseBrowserState(serializeBrowserState(state));
    expect(restored.lastSide).toBe("right");
    expect(restored.docks[0].sessionId).toBe("chat-a");
    expect(restored.docks[0].pane.files[0].browser).toEqual({
      url: "https://a.test/",
      title: "A",
    });
  });

  it("drops malformed docks and tabs", () => {
    expect(parseBrowserState("not json").docks).toEqual([]);
    const raw = JSON.stringify({
      docks: [
        { sessionId: "a", side: "middle", pane: { files: [] } },
        { projectPath: "/old", side: "left", pane: { files: [] } },
        {
          sessionId: "b",
          side: "left",
          size: 400,
          open: true,
          pane: {
            id: "p",
            activeFileId: "gone",
            files: [
              { id: "../bad", browser: { url: "https://x.test" } },
              {
                id: "ok-1",
                path: "https://y.test",
                cwd: "",
                browser: { url: "https://y.test" },
              },
            ],
          },
        },
      ],
    });
    const { docks } = parseBrowserState(raw);
    expect(docks).toHaveLength(1);
    expect(docks[0].pane.files.map((file) => file.id)).toEqual(["ok-1"]);
    expect(docks[0].pane.activeFileId).toBe("ok-1");
  });
});

describe("load generations", () => {
  it("settles only while no newer load has started", () => {
    const id = openBrowserTab("https://a.test", { sessionId: "gen-s" })!;
    expect(settleLoad(id, loadGeneration(id))).toBe(true);
    const seen = loadGeneration(id);
    noteLoadStarted(id);
    expect(loadGeneration(id)).toBe(seen + 1);
    expect(settleLoad(id, seen)).toBe(false);
    forgetBrowserSessions(["gen-s"]);
  });

  it("ignores starts for tabs that are not in the store", () => {
    noteLoadStarted("ghost-tab");
    expect(loadGeneration("ghost-tab")).toBe(0);
  });

  it("forgets generations when tabs close, close others, or sessions go", () => {
    const a = openBrowserTab("https://a.test", { sessionId: "gen-c" })!;
    const b = openBrowserTab("https://b.test", { sessionId: "gen-c" })!;
    const c = openBrowserTab("https://c.test", { sessionId: "gen-c" })!;
    for (const id of [a, b, c]) noteLoadStarted(id);
    closeBrowserTab(a);
    expect(loadGeneration(a)).toBe(0);
    closeOtherBrowserTabs(b);
    expect(loadGeneration(b)).toBe(1);
    expect(loadGeneration(c)).toBe(0);
    forgetBrowserSessions(["gen-c"]);
    expect(loadGeneration(b)).toBe(0);
  });
});
