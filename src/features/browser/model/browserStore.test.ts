import { describe, expect, it } from "vitest";
import {
  addBrowserTab,
  newBrowserTab,
  parseBrowserState,
  patchBrowserTabIn,
  serializeBrowserState,
  toggleBrowserDock,
  type BrowserState,
} from "./browserStore";

const empty: BrowserState = {
  docks: [],
  projectPath: "/work/app",
  lastSide: "right",
};

describe("browser docks", () => {
  it("toggle opens a blank tab on the right, then hides and shows", () => {
    const opened = toggleBrowserDock(empty);
    expect(opened.docks).toHaveLength(1);
    expect(opened.docks[0]).toMatchObject({
      projectPath: "/work/app",
      side: "right",
      open: true,
    });
    expect(opened.docks[0].pane.files[0].browser?.url).toBe("about:blank");

    const hidden = toggleBrowserDock(opened);
    expect(hidden.docks[0].open).toBe(false);
    expect(toggleBrowserDock(hidden).docks[0].open).toBe(true);
  });

  it("opens a page's new window right after its opener", () => {
    const a = newBrowserTab("https://a.test/", "/work/app");
    const b = newBrowserTab("https://b.test/", "/work/app");
    const c = newBrowserTab("https://c.test/", "/work/app");
    let state = addBrowserTab(empty, a);
    state = addBrowserTab(state, b);
    state = addBrowserTab(state, c, undefined, { afterId: a.id });
    expect(state.docks[0].pane.files.map((file) => file.id)).toEqual([
      a.id,
      c.id,
      b.id,
    ]);
    expect(state.docks[0].pane.activeFileId).toBe(c.id);
  });

  it("keeps each project's tabs in its own dock", () => {
    const a = newBrowserTab("https://a.test/", "/work/app");
    const b = newBrowserTab("https://b.test/", "/work/other");
    const state = addBrowserTab(addBrowserTab(empty, a), b, "/work/other");
    expect(state.docks.map((dock) => dock.projectPath)).toEqual([
      "/work/app",
      "/work/other",
    ]);
  });

  it("patches page state and leaves unchanged tabs alone", () => {
    const tab = newBrowserTab("https://a.test/", "/work/app");
    const state = addBrowserTab(empty, tab);
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
});

describe("browser persistence", () => {
  it("round-trips docks without live page state", () => {
    const tab = newBrowserTab("https://a.test/", "/work/app");
    const state = patchBrowserTabIn(addBrowserTab(empty, tab), tab.id, {
      title: "A",
      loading: true,
    });
    const restored = parseBrowserState(serializeBrowserState(state));
    expect(restored.lastSide).toBe("right");
    expect(restored.docks[0].pane.files[0].browser).toEqual({
      url: "https://a.test/",
      title: "A",
    });
  });

  it("drops malformed docks and tabs", () => {
    expect(parseBrowserState("not json").docks).toEqual([]);
    const raw = JSON.stringify({
      docks: [
        { projectPath: "/a", side: "middle", pane: { files: [] } },
        {
          projectPath: "/b",
          side: "left",
          size: 400,
          open: true,
          pane: {
            id: "p",
            activeFileId: "gone",
            files: [
              { id: "../bad", browser: { url: "https://x.test" } },
              { id: "ok-1", path: "https://y.test", cwd: "/b", browser: { url: "https://y.test" } },
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
