import { describe, expect, it } from "vitest";
import {
  closeLeaf,
  editorTabKey,
  isTerminalTab,
  layoutLeaves,
  layoutSashes,
  leaf,
  newFileTab,
  newPlanTab,
  newTab,
  newTerminalFile,
  newTerminalWorkspaceTab,
  nextTerminalTitle,
  isolateTerminalPanes,
  movePane,
  openEditorTab,
  openTerminalTab,
  paneEdgeFromPoint,
  splitPane,
  splitSizesAtBoundary,
} from "./layout";

describe("splitSizesAtBoundary", () => {
  it("moves only the adjacent panes and preserves their total", () => {
    const result = splitSizesAtBoundary([0.2, 0.3, 0.5], 1, 0.7);
    expect(result[0]).toBe(0.2);
    expect(result[1]).toBeCloseTo(0.5);
    expect(result[2]).toBeCloseTo(0.3);
  });

  it("clamps both panes to the minimum size", () => {
    const right = splitSizesAtBoundary([0.5, 0.5], 0, 0.99);
    expect(right[0]).toBeCloseTo(0.92);
    expect(right[1]).toBeCloseTo(0.08);

    const left = splitSizesAtBoundary([0.5, 0.5], 0, 0.01);
    expect(left[0]).toBeCloseTo(0.08);
    expect(left[1]).toBeCloseTo(0.92);
  });

  it("leaves invalid boundaries unchanged", () => {
    const sizes = [0.5, 0.5];
    expect(splitSizesAtBoundary(sizes, 2, 0.5)).toBe(sizes);
  });
});

describe("layoutLeaves", () => {
  it("keeps a single pane filling the tab", () => {
    const leaves = layoutLeaves(leaf("a"));
    expect(leaves).toEqual([
      { id: "a", rect: { x: 0, y: 0, w: 1, h: 1 }, axis: "x" },
    ]);
  });

  it("places a right split side by side without changing leaf ids", () => {
    const tree = splitPane(leaf("a"), "a", "right", "b");
    const leaves = layoutLeaves(tree);
    expect(leaves.map((pane) => pane.id)).toEqual(["a", "b"]);
    expect(leaves[0].rect).toEqual({ x: 0, y: 0, w: 0.5, h: 1 });
    expect(leaves[1].rect).toEqual({ x: 0.5, y: 0, w: 0.5, h: 1 });
  });
});

describe("layoutSashes", () => {
  it("puts a sash on the shared edge of a right split", () => {
    const tree = splitPane(leaf("a"), "a", "right", "b");
    const sashes = layoutSashes(tree);
    expect(sashes).toHaveLength(1);
    expect(sashes[0]?.index).toBe(0);
    expect(sashes[0]?.dir).toBe("right");
    expect(sashes[0]?.group).toEqual({ x: 0, y: 0, w: 1, h: 1 });
  });
});

describe("editorTabKey", () => {
  it("keeps a working-tree tab distinct from a normal file tab", () => {
    const cwd = "/repo";
    const path = "/repo/EventStore.swift";
    expect(editorTabKey(newFileTab(path, cwd))).toBe(`file:${path}`);
    expect(editorTabKey(newFileTab(path, cwd, true))).toBe(`review:${path}`);
    expect(editorTabKey(newPlanTab("s", "b", "Plan", cwd))).toBe("plan:b");
    const terminal = newTerminalFile(cwd);
    expect(editorTabKey(terminal)).toBe(`terminal:${terminal.id}`);
    expect(isTerminalTab(terminal)).toBe(true);
    expect(isTerminalTab(newFileTab(path, cwd))).toBe(false);
  });

  it("separates a session review from the project review of one file", () => {
    const cwd = "/repo";
    const path = "/repo/EventStore.swift";
    expect(editorTabKey(newFileTab(path, cwd, true, "s1"))).toBe(
      `review:s1:${path}`,
    );
    expect(editorTabKey(newFileTab(path, cwd, true, "s2"))).not.toBe(
      editorTabKey(newFileTab(path, cwd, true, "s1")),
    );
    // A session id without review must not change the plain file tab.
    expect(editorTabKey(newFileTab(path, cwd, false, "s1"))).toBe(
      `file:${path}`,
    );
  });
});

describe("openTerminalTab", () => {
  it("occupies a session pane instead of splitting a leftover chat", () => {
    const tab = newTab("session-a");
    const file = newTerminalFile("/repo");
    const next = openTerminalTab(tab, file, "session-a");
    expect(layoutLeaves(next.layout).map((pane) => pane.id)).toEqual([
      next.terminalPanes[0]?.id,
    ]);
    expect(next.focusedId).toBe(next.terminalPanes[0]?.id);
    expect(next.editorPanes).toEqual([]);
    expect(next.terminalPanes[0]?.files).toEqual([file]);
  });

  it("splits a terminal pane below the session when no terminal pane exists", () => {
    const tab = newTab("session-a");
    const file = newTerminalFile("/repo");
    const next = openTerminalTab(tab, file);
    const leaves = layoutLeaves(next.layout);
    expect(leaves.map((pane) => pane.id)).toEqual([
      "session-a",
      next.terminalPanes[0]?.id,
    ]);
    expect(leaves[0]?.rect).toEqual({ x: 0, y: 0, w: 1, h: 0.5 });
    expect(leaves[1]?.rect).toEqual({ x: 0, y: 0.5, w: 1, h: 0.5 });
    expect(next.editorPanes).toEqual([]);
    expect(next.terminalPanes[0]?.files).toEqual([file]);
  });

  it("keeps terminals out of the file pane tab strip", () => {
    const withFile = openEditorTab(
      newTab("session-a"),
      newFileTab("/repo/App.tsx", "/repo"),
    );
    const first = newTerminalFile("/repo");
    const withTerminal = openTerminalTab(withFile, first);
    const extra = newTerminalFile("/repo", nextTerminalTitle(withTerminal, "/repo"));
    const next = openTerminalTab(withTerminal, extra);
    expect(next.editorPanes).toHaveLength(1);
    expect(next.editorPanes[0]?.files.map((file) => file.path)).toEqual([
      "/repo/App.tsx",
    ]);
    expect(next.terminalPanes).toHaveLength(1);
    expect(next.terminalPanes[0]?.files.map((file) => file.id)).toEqual([
      first.id,
      extra.id,
    ]);
    expect(extra.path).toBe("repo 2");
    expect(layoutLeaves(next.layout)).toHaveLength(3);
  });
});

describe("closeLeaf", () => {
  it("keeps a file pane when the last chat is closed", () => {
    const file = newFileTab("/repo/App.tsx", "/repo");
    const tab = openEditorTab(newTab("session-a"), file);
    const next = closeLeaf(tab, "session-a");
    expect(next).not.toBeNull();
    expect(layoutLeaves(next!.layout).map((pane) => pane.id)).toEqual([
      next!.editorPanes[0]?.id,
    ]);
    expect(next!.focusedId).toBe(next!.editorPanes[0]?.id);
    expect(next!.editorPanes[0]?.files).toEqual([file]);
  });

  it("keeps a terminal pane when the last chat is closed", () => {
    const file = newTerminalFile("/repo");
    const tab = openTerminalTab(newTab("session-a"), file);
    const next = closeLeaf(tab, "session-a");
    expect(next).not.toBeNull();
    expect(layoutLeaves(next!.layout).map((pane) => pane.id)).toEqual([
      next!.terminalPanes[0]?.id,
    ]);
    expect(next!.focusedId).toBe(next!.terminalPanes[0]?.id);
    expect(next!.terminalPanes[0]?.files).toEqual([file]);
  });

  it("returns null when closing the last remaining pane", () => {
    expect(closeLeaf(newTab("session-a"), "session-a")).toBeNull();
  });
});

describe("openEditorTab", () => {
  it("does not open files into a terminal pane", () => {
    const terminal = openTerminalTab(
      newTab("session-a"),
      newTerminalFile("/repo"),
    );
    const next = openEditorTab(
      terminal,
      newFileTab("/repo/App.tsx", "/repo"),
    );
    expect(next.terminalPanes[0]?.files.every(isTerminalTab)).toBe(true);
    expect(next.editorPanes[0]?.files.map((file) => file.path)).toEqual([
      "/repo/App.tsx",
    ]);
    expect(layoutLeaves(next.layout)).toHaveLength(3);
  });
});

describe("newTerminalWorkspaceTab", () => {
  it("creates a tab whose only leaf is the terminal pane", () => {
    const file = newTerminalFile("/repo");
    const tab = newTerminalWorkspaceTab(file);
    expect(layoutLeaves(tab.layout).map((pane) => pane.id)).toEqual([
      tab.terminalPanes[0]?.id,
    ]);
    expect(tab.focusedId).toBe(tab.terminalPanes[0]?.id);
    expect(tab.editorPanes).toEqual([]);
    expect(tab.terminalPanes[0]?.files).toEqual([file]);
  });
});

describe("isolateTerminalPanes", () => {
  it("splits mixed file and terminal tabs into separate panes", () => {
    const file = newFileTab("/repo/App.tsx", "/repo");
    const terminal = newTerminalFile("/repo");
    const mixed = openEditorTab(newTab("session-a"), file);
    const pane = mixed.editorPanes[0];
    if (!pane) throw new Error("expected editor pane");
    const tab = {
      ...mixed,
      editorPanes: [
        {
          ...pane,
          files: [file, terminal],
          activeFileId: terminal.id,
        },
      ],
    };
    const next = isolateTerminalPanes(tab);
    expect(next.editorPanes[0]?.files.map((entry) => entry.id)).toEqual([
      file.id,
    ]);
    expect(next.terminalPanes[0]?.files.map((entry) => entry.id)).toEqual([
      terminal.id,
    ]);
    expect(next.focusedId).toBe(next.terminalPanes[0]?.id);
    expect(layoutLeaves(next.layout)).toHaveLength(3);
  });
});

describe("paneEdgeFromPoint", () => {
  const rect = { left: 0, top: 0, width: 100, height: 100 };

  it("picks the nearest edge from the pane center", () => {
    expect(paneEdgeFromPoint(20, 50, rect)).toBe("left");
    expect(paneEdgeFromPoint(80, 50, rect)).toBe("right");
    expect(paneEdgeFromPoint(50, 20, rect)).toBe("top");
    expect(paneEdgeFromPoint(50, 80, rect)).toBe("bottom");
  });
});

describe("movePane", () => {
  it("reorders siblings along a horizontal split", () => {
    const tree = splitPane(leaf("a"), "a", "right", "b");
    const next = movePane(tree, "a", "b", "right");
    const leaves = layoutLeaves(next);
    expect(leaves.map((pane) => pane.id)).toEqual(["b", "a"]);
    expect(leaves[0]?.rect).toEqual({ x: 0, y: 0, w: 0.5, h: 1 });
    expect(leaves[1]?.rect).toEqual({ x: 0.5, y: 0, w: 0.5, h: 1 });
  });

  it("stacks a dragged pane under its sibling", () => {
    const tree = splitPane(leaf("a"), "a", "right", "b");
    const next = movePane(tree, "b", "a", "bottom");
    const leaves = layoutLeaves(next);
    expect(leaves.map((pane) => pane.id)).toEqual(["a", "b"]);
    expect(leaves[0]?.rect).toEqual({ x: 0, y: 0, w: 1, h: 0.5 });
    expect(leaves[1]?.rect).toEqual({ x: 0, y: 0.5, w: 1, h: 0.5 });
  });

  it("nests a pane under one column of a row", () => {
    const row = splitPane(
      splitPane(leaf("a"), "a", "right", "b"),
      "b",
      "right",
      "c",
    );
    const next = movePane(row, "c", "a", "bottom");
    const leaves = layoutLeaves(next);
    expect(leaves.map((pane) => pane.id)).toEqual(["a", "c", "b"]);
    expect(leaves[0]?.rect).toEqual({ x: 0, y: 0, w: 0.5, h: 0.5 });
    expect(leaves[1]?.rect).toEqual({ x: 0, y: 0.5, w: 0.5, h: 0.5 });
    expect(leaves[2]?.rect).toEqual({ x: 0.5, y: 0, w: 0.5, h: 1 });
  });

  it("inserts into an existing vertical split on a matching edge", () => {
    const stacked = splitPane(leaf("a"), "a", "down", "b");
    const row = splitPane(stacked, "a", "right", "c");
    const next = movePane(row, "c", "a", "bottom");
    const leaves = layoutLeaves(next);
    expect(leaves.map((pane) => pane.id)).toEqual(["a", "c", "b"]);
    expect(leaves[0]?.rect).toEqual({ x: 0, y: 0, w: 1, h: 0.25 });
    expect(leaves[1]?.rect).toEqual({ x: 0, y: 0.25, w: 1, h: 0.25 });
    expect(leaves[2]?.rect).toEqual({ x: 0, y: 0.5, w: 1, h: 0.5 });
  });
});
