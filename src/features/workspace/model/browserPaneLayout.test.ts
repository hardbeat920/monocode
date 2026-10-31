import { describe, expect, it } from "vitest";
import {
  browserPanePlacement,
  layoutLeaves,
  newBrowserPaneTab,
  newFileTab,
  newTab,
  openBrowserPane,
  openEditorTab,
  movePane,
  ownBrowserPaneTab,
} from "./layout";

const browser = (tab: string) =>
  newBrowserPaneTab("Checkout", "/repo", { sessionId: "s1", tab });

describe("browser panes", () => {
  it("mark a tab the user opened as theirs, also when it shows already", () => {
    const shown = openBrowserPane(newTab("s1"), browser("t1"), "s1");
    const own = newBrowserPaneTab("New tab", "/repo", {
      sessionId: "s1",
      tab: "t1",
      own: true,
    });
    const marked = ownBrowserPaneTab(shown, own);
    expect(marked.editorPanes[0].files[0].browser).toEqual({
      sessionId: "s1",
      tab: "t1",
      own: true,
    });
    // Nothing shows it yet: the tab is left as it is.
    const other = browser("t2");
    expect(ownBrowserPaneTab(shown, other)).toBe(shown);
  });

  it("open in a pane of their own on the right the first time", () => {
    const tab = openBrowserPane(newTab("s1"), browser("t1"), "s1");
    const [left, right] = layoutLeaves(tab.layout);
    expect(left.id).toBe("s1");
    expect(right.id).toBe(tab.editorPanes[0].id);
    expect(right.rect.w).toBeCloseTo(0.5);
    expect(tab.focusedId).toBe(tab.editorPanes[0].id);
  });

  it("open where the user last left one, at that share", () => {
    const tab = openBrowserPane(newTab("s1"), browser("t1"), "s1", {
      edge: "left",
      size: 0.35,
    });
    const [left, right] = layoutLeaves(tab.layout);
    expect(left.id).toBe(tab.editorPanes[0].id);
    expect(left.rect.w).toBeCloseTo(0.35);
    expect(right.id).toBe("s1");
    expect(
      browserPanePlacement(tab.layout, tab.editorPanes[0].id, "s1"),
    ).toEqual({ edge: "left", size: 0.35 });
  });

  it("read the side back after the user moves the pane", () => {
    const opened = openBrowserPane(newTab("s1"), browser("t1"), "s1");
    const pane = opened.editorPanes[0].id;
    const below = movePane(opened.layout, pane, "s1", "bottom");
    expect(browserPanePlacement(below, pane, "s1")).toEqual({
      edge: "bottom",
      size: 0.5,
    });
  });

  it("keep files out of the browser pane and browser tabs together", () => {
    const withFile = openEditorTab(
      newTab("s1"),
      newFileTab("/repo/README.md", "/repo"),
    );
    const first = openBrowserPane(withFile, browser("t1"), "s1");
    expect(first.editorPanes).toHaveLength(2);
    const second = openBrowserPane(first, browser("t2"), "s1");
    expect(second.editorPanes).toHaveLength(2);
    expect(
      second.editorPanes[1].files.map((file) => file.browser?.tab),
    ).toEqual(["t1", "t2"]);
    // The same tab again only focuses it.
    const again = openBrowserPane(second, browser("t1"), "s1");
    expect(again.editorPanes[1].files).toHaveLength(2);
    expect(again.editorPanes[1].activeFileId).toBe(
      second.editorPanes[1].files[0].id,
    );
  });
});
