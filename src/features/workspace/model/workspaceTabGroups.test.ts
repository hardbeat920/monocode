import { describe, expect, it } from "vitest";
import {
  leafIds,
  newFileTab,
  newTerminalFile,
  newTab,
  splitPane,
  type WorkspaceTab,
} from "./layout";
import { planProjectReturn } from "../../projects/model/projectReturn";
import type { Session } from "../../sessions/model/session";
import {
  applyDetachPaneToTab,
  applyPlaceTabOnPane,
  applyPlaceSessionOnPane,
  filterTabsForProject,
  findOpenSessionTab,
  findTabForProject,
  openAddToChatSessionPane,
  planWorkspaceTabClose,
  replaceGroupInTabOrder,
  workspaceTabProject,
  focusedWorkspaceTabCwd,
  workspaceTabCwd,
} from "./workspaceTabGroups";

function session(id: string, cwd: string): Session {
  return {
    id,
    cwd,
    harness: "cursor",
    title: "",
    blocks: [],
    busy: false,
    model: "",
  };
}

function tab(id: string, sessionId: string): WorkspaceTab {
  return { ...newTab(sessionId), id };
}

describe("focusedWorkspaceTabCwd", () => {
  it.each(["editor", "terminal"] as const)(
    "keeps a worktree-only %s tab under its owning project",
    (kind) => {
      const cwd = "/repo-worktrees/feature";
      const file = kind === "editor"
        ? newFileTab(`${cwd}/readme.md`, cwd, false, undefined, "/repo")
        : newTerminalFile(cwd, undefined, "/repo");
      const pane = { id: "surface", files: [file], activeFileId: file.id };
      const worktreeTab = {
        ...tab("tree-tab", pane.id),
        editorPanes: kind === "editor" ? [pane] : [],
        terminalPanes: kind === "terminal" ? [pane] : [],
      };
      expect(workspaceTabCwd(worktreeTab, [])).toBe("/repo");
      expect(focusedWorkspaceTabCwd(worktreeTab, [])).toBe("/repo");
      expect(planProjectReturn({
        tabs: [worktreeTab], sessions: [], memory: new Map(),
        activeTabId: "elsewhere", projectPath: "/repo",
      })).toMatchObject({ action: "activate", tabId: "tree-tab" });
    },
  );

  it.each(["editor", "terminal"] as const)(
    "uses the restored %s pane's project instead of the first chat's project",
    (kind) => {
      const sessions = [session("chat", "/alpha")];
      const file =
        kind === "editor"
          ? newFileTab("/beta/readme.md", "/beta")
          : newTerminalFile("/beta");
      const pane = { id: "surface", files: [file], activeFileId: file.id };
      const mixed: WorkspaceTab = {
        ...tab("mixed", "chat"),
        layout: splitPane(newTab("chat").layout, "chat", "right", pane.id),
        editorPanes: kind === "editor" ? [pane] : [],
        terminalPanes: kind === "terminal" ? [pane] : [],
      };
      const decision = planProjectReturn({
        tabs: [mixed],
        sessions,
        memory: new Map([["/beta", pane.id]]),
        activeTabId: mixed.id,
        projectPath: "/beta",
      });
      expect(decision).toEqual({
        action: "activate",
        tabId: mixed.id,
        paneId: pane.id,
      });
      if (decision.action !== "activate" || !decision.paneId) {
        throw new Error("Expected pane activation");
      }
      const focusedTab = { ...mixed, focusedId: decision.paneId };
      expect(workspaceTabCwd(focusedTab, sessions)).toBe("/alpha");
      expect(focusedWorkspaceTabCwd(focusedTab, sessions)).toBe("/beta");
      expect(mixed.focusedId).toBe("chat");
    },
  );

  it("prefers the focused chat over the first chat", () => {
    const mixed = {
      ...tab("mixed", "first"),
      layout: splitPane(newTab("first").layout, "first", "right", "second"),
      focusedId: "second",
    };
    expect(
      focusedWorkspaceTabCwd(mixed, [
        session("first", "/alpha"),
        session("second", "/beta"),
      ]),
    ).toBe("/beta");
  });

  it("retains the tab fallback when the focused pane has no cwd", () => {
    const mixed = { ...tab("mixed", "chat"), focusedId: "missing" };
    expect(focusedWorkspaceTabCwd(mixed, [session("chat", "/alpha")])).toBe(
      "/alpha",
    );
    expect(focusedWorkspaceTabCwd(mixed, [])).toBeNull();
  });
});

describe("findOpenSessionTab", () => {
  it("does not focus a ghost tab whose session has been parked", () => {
    const ghost = tab("ghost-tab", "parked-session");
    expect(findOpenSessionTab([ghost], [], "parked-session")).toBeUndefined();
    expect(
      findOpenSessionTab(
        [ghost],
        [session("parked-session", "/workspace")],
        "parked-session",
      ),
    ).toBe(ghost);
  });
});

describe("openAddToChatSessionPane", () => {
  it("opens and focuses a chat beside the focused file pane", () => {
    const file = newFileTab("/workspace/readme.md", "/workspace");
    const pane = { id: "editor", files: [file], activeFileId: file.id };
    const fileOnly: WorkspaceTab = {
      ...newTab(pane.id),
      id: "file-tab",
      editorPanes: [pane],
      diffFocused: true,
    };

    const opened = openAddToChatSessionPane({
      tab: fileOnly,
      sessions: [],
      sessionId: "new-chat",
    });

    expect(leafIds(opened!.layout)).toEqual([pane.id, "new-chat"]);
    expect(opened?.focusedId).toBe("new-chat");
    expect(opened?.diffFocused).toBe(false);
    expect(opened?.editorPanes).toEqual([pane]);
  });

  it("leaves add-to-chat routing to an existing session pane", () => {
    const chatTab = tab("chat-tab", "existing-chat");
    expect(
      openAddToChatSessionPane({
        tab: chatTab,
        sessions: [session("existing-chat", "/workspace")],
        sessionId: "unused-chat",
      }),
    ).toBeNull();
  });
});

describe("workspaceTabProject", () => {
  it("reads project from the tab session cwd", () => {
    const workspace = tab("t1", "s1");
    const sessions = [session("s1", "/Users/me/agent-terminal")];
    expect(workspaceTabProject(workspace, sessions)).toBe("agent-terminal");
  });
});

describe("findTabForProject", () => {
  it("matches a tab by project path, ignoring trailing slashes", () => {
    const tabs = [tab("t1", "s1"), tab("t2", "s2")];
    const sessions = [
      session("s1", "/tmp/alpha"),
      session("s2", "/tmp/beta"),
    ];
    expect(findTabForProject(tabs, sessions, "/tmp/beta/")?.id).toBe("t2");
  });

  it("returns undefined when no open tab belongs to the project", () => {
    const tabs = [tab("t1", "s1")];
    const sessions = [session("s1", "/tmp/alpha")];
    expect(findTabForProject(tabs, sessions, "/tmp/beta")).toBeUndefined();
  });
});

describe("filterTabsForProject", () => {
  it("keeps only tabs that belong to the project", () => {
    const tabs = [tab("t1", "s1"), tab("t2", "s2"), tab("t3", "s3")];
    const sessions = [
      session("s1", "/tmp/alpha"),
      session("s2", "/tmp/beta"),
      session("s3", "/tmp/beta"),
    ];
    expect(
      filterTabsForProject(tabs, sessions, "/tmp/beta").map((tab) => tab.id),
    ).toEqual(["t2", "t3"]);
  });
});

describe("planWorkspaceTabClose", () => {
  const sessions = [
    session("m1", "/projects/monocode"),
    session("r1", "/projects/ruler"),
    session("m2", "/projects/monocode"),
  ];
  const tabs = [tab("tm1", "m1"), tab("tr1", "r1"), tab("tm2", "m2")];

  it("uses the global neighbor in workspace scope", () => {
    expect(
      planWorkspaceTabClose({
        tabs,
        sessions,
        closingTabId: "tm2",
        scope: "workspace",
      }),
    ).toEqual({ action: "close", nextActiveTabId: "tr1" });
  });

  it("prefers the previous same-project tab in project scope", () => {
    expect(
      planWorkspaceTabClose({
        tabs,
        sessions,
        closingTabId: "tm2",
        scope: "project",
      }),
    ).toEqual({ action: "close", nextActiveTabId: "tm1" });
  });

  it("uses the next same-project tab when none exists to the left", () => {
    expect(
      planWorkspaceTabClose({
        tabs,
        sessions,
        closingTabId: "tm1",
        scope: "project",
      }),
    ).toEqual({ action: "close", nextActiveTabId: "tm2" });
  });

  it("keeps the last tab of a project instead of jumping to another", () => {
    expect(
      planWorkspaceTabClose({
        tabs: tabs.slice(0, 2),
        sessions,
        closingTabId: "tm1",
        scope: "project",
      }),
    ).toEqual({ action: "keep" });
  });

  it("still jumps across projects in workspace scope when a project is emptied", () => {
    expect(
      planWorkspaceTabClose({
        tabs: tabs.slice(0, 2),
        sessions,
        closingTabId: "tm1",
        scope: "workspace",
      }),
    ).toEqual({ action: "close", nextActiveTabId: "tr1" });
  });

  it("uses the global neighbor for a projectless tab", () => {
    const projectlessSessions = [
      ...sessions,
      session("blank1", "~"),
      session("blank2", "~"),
    ];
    expect(
      planWorkspaceTabClose({
        tabs: [tab("projectless", "blank1"), tabs[1]],
        sessions: projectlessSessions,
        closingTabId: "projectless",
        scope: "project",
      }),
    ).toEqual({ action: "close", nextActiveTabId: "tr1" });
    expect(
      planWorkspaceTabClose({
        tabs: [tab("blank1", "blank1"), tabs[1], tab("blank2", "blank2")],
        sessions: projectlessSessions,
        closingTabId: "blank2",
        scope: "project",
      }),
    ).toEqual({ action: "close", nextActiveTabId: "tr1" });
  });

  it("keeps the sole tab or an unknown tab", () => {
    expect(
      planWorkspaceTabClose({
        tabs: [tabs[0]],
        sessions,
        closingTabId: "tm1",
        scope: "workspace",
      }),
    ).toEqual({ action: "keep" });
    expect(
      planWorkspaceTabClose({
        tabs,
        sessions,
        closingTabId: "missing",
        scope: "project",
      }),
    ).toEqual({ action: "keep" });
  });
});

describe("applyPlaceSessionOnPane", () => {
  const sessions = [
    session("m1", "/projects/monocode"),
    session("m2", "/projects/monocode"),
    session("r1", "/projects/ruler"),
  ];

  function replacementFrom(seed: Session | undefined): Session {
    return session("replacement", seed?.cwd ?? "/tmp/fallback");
  }

  it("splits the target pane toward the drop edge", () => {
    const next = applyPlaceSessionOnPane({
      tabs: [tab("tm1", "m1")],
      sessions,
      sessionId: "m2",
      targetId: "m1",
      edge: "right",
      replaceTarget: false,
      scope: "workspace",
      createReplacement: replacementFrom,
    });
    expect(next?.activeTabId).toBe("tm1");
    expect(next?.tabs[0]?.focusedId).toBe("m2");
    expect(leafIds(next!.tabs[0]!.layout)).toEqual(["m1", "m2"]);
  });

  it("replaces a blank target instead of splitting it", () => {
    const blank = session("blank", "/projects/monocode");
    const next = applyPlaceSessionOnPane({
      tabs: [tab("tm1", "blank")],
      sessions: [...sessions, blank],
      sessionId: "m2",
      targetId: "blank",
      edge: "right",
      replaceTarget: true,
      scope: "workspace",
      createReplacement: replacementFrom,
    });
    expect(leafIds(next!.tabs[0]!.layout)).toEqual(["m2"]);
    expect(next?.sessions.map((entry) => entry.id)).toEqual([
      "m1",
      "m2",
      "r1",
    ]);
  });

  it("relocates a session from another tab and closes that tab", () => {
    const next = applyPlaceSessionOnPane({
      tabs: [tab("tm1", "m1"), tab("tm2", "m2")],
      sessions,
      sessionId: "m2",
      targetId: "m1",
      edge: "left",
      replaceTarget: false,
      scope: "workspace",
      createReplacement: replacementFrom,
    });
    expect(next?.tabs.map((entry) => entry.id)).toEqual(["tm1"]);
    expect(leafIds(next!.tabs[0]!.layout)).toEqual(["m2", "m1"]);
  });

  it("keeps the last tab of a project and fills it with a replacement", () => {
    const next = applyPlaceSessionOnPane({
      tabs: [tab("tr1", "r1"), tab("tm1", "m1")],
      sessions,
      sessionId: "m1",
      targetId: "r1",
      edge: "right",
      replaceTarget: false,
      scope: "project",
      createReplacement: replacementFrom,
    });
    expect(next?.tabs.map((entry) => entry.id)).toEqual(["tr1", "tm1"]);
    expect(leafIds(next!.tabs[0]!.layout)).toEqual(["r1", "m1"]);
    expect(next?.tabs[1]?.focusedId).toBe("replacement");
  });
});

describe("applyPlaceTabOnPane", () => {
  const sessions = [
    session("target", "/projects/monocode"),
    session("source", "/projects/monocode"),
    session("other", "/projects/monocode"),
  ];

  it("turns a separate tab into a split beside the target pane", () => {
    const next = applyPlaceTabOnPane({
      tabs: [tab("target-tab", "target"), tab("source-tab", "source")],
      sessions,
      sourceTabId: "source-tab",
      targetId: "target",
      edge: "left",
      replaceTarget: false,
    });

    expect(next?.activeTabId).toBe("target-tab");
    expect(next?.tabs.map((entry) => entry.id)).toEqual(["target-tab"]);
    expect(leafIds(next!.tabs[0]!.layout)).toEqual(["source", "target"]);
    expect(next?.tabs[0]?.focusedId).toBe("source");
  });

  it("keeps every pane and the nested layout of the dragged tab", () => {
    const file = newFileTab(
      "/projects/monocode/readme.md",
      "/projects/monocode",
    );
    const pane = { id: "editor", files: [file], activeFileId: file.id };
    const source: WorkspaceTab = {
      ...tab("source-tab", "source"),
      layout: splitPane(newTab("source").layout, "source", "down", pane.id),
      focusedId: pane.id,
      editorPanes: [pane],
    };
    const next = applyPlaceTabOnPane({
      tabs: [tab("target-tab", "target"), source],
      sessions,
      sourceTabId: source.id,
      targetId: "target",
      edge: "right",
      replaceTarget: false,
    });

    expect(leafIds(next!.tabs[0]!.layout)).toEqual([
      "target",
      "source",
      "editor",
    ]);
    expect(next?.tabs[0]?.layout).toMatchObject({
      type: "split",
      dir: "right",
      children: [
        { type: "leaf", id: "target" },
        { type: "split", dir: "down" },
      ],
    });
    expect(next?.tabs[0]?.editorPanes).toEqual([pane]);
    expect(next?.focusedId).toBe("editor");
  });

  it("replaces a blank target without leaving its session mounted", () => {
    const blank = session("blank", "/projects/monocode");
    const next = applyPlaceTabOnPane({
      tabs: [tab("target-tab", "blank"), tab("source-tab", "source")],
      sessions: [...sessions, blank],
      sourceTabId: "source-tab",
      targetId: blank.id,
      edge: "bottom",
      replaceTarget: true,
    });

    expect(leafIds(next!.tabs[0]!.layout)).toEqual(["source"]);
    expect(next?.sessions.some((entry) => entry.id === blank.id)).toBe(false);
  });
});

describe("applyDetachPaneToTab", () => {
  it("turns one chat pane into a separate tab at the drop position", () => {
    const source: WorkspaceTab = {
      ...tab("source-tab", "first"),
      layout: splitPane(newTab("first").layout, "first", "right", "second"),
      focusedId: "second",
    };
    const target = tab("target-tab", "third");
    const next = applyDetachPaneToTab({
      tabs: [source, target],
      paneId: "second",
      targetTabId: target.id,
      position: "before",
      createTabId: () => "detached-tab",
    });

    expect(next?.activeTabId).toBe("detached-tab");
    expect(next?.focusedId).toBe("second");
    expect(next?.tabs.map((entry) => entry.id)).toEqual([
      "source-tab",
      "detached-tab",
      "target-tab",
    ]);
    expect(leafIds(next!.tabs[0]!.layout)).toEqual(["first"]);
    expect(leafIds(next!.tabs[1]!.layout)).toEqual(["second"]);
  });

  it.each(["editor", "terminal"] as const)(
    "moves the complete %s pane metadata into the new tab",
    (kind) => {
      const file =
        kind === "editor"
          ? newFileTab("/projects/monocode/readme.md", "/projects/monocode")
          : newTerminalFile("/projects/monocode");
      const pane = { id: `${kind}-pane`, files: [file], activeFileId: file.id };
      const source: WorkspaceTab = {
        ...tab("source-tab", "chat"),
        layout: splitPane(newTab("chat").layout, "chat", "down", pane.id),
        editorPanes: kind === "editor" ? [pane] : [],
        terminalPanes: kind === "terminal" ? [pane] : [],
      };
      const next = applyDetachPaneToTab({
        tabs: [source, tab("target-tab", "other")],
        paneId: pane.id,
        targetTabId: "target-tab",
        position: "after",
        createTabId: () => "detached-tab",
      });

      expect(next?.tabs.map((entry) => entry.id)).toEqual([
        "source-tab",
        "target-tab",
        "detached-tab",
      ]);
      expect(next?.tabs[0]?.editorPanes).toEqual([]);
      expect(next?.tabs[0]?.terminalPanes).toEqual([]);
      expect(next?.tabs[2]?.editorPanes).toEqual(
        kind === "editor" ? [pane] : [],
      );
      expect(next?.tabs[2]?.terminalPanes).toEqual(
        kind === "terminal" ? [pane] : [],
      );
    },
  );

  it("does not detach the only pane in a tab", () => {
    expect(
      applyDetachPaneToTab({
        tabs: [tab("source-tab", "only")],
        paneId: "only",
        targetTabId: "source-tab",
        position: "after",
      }),
    ).toBeNull();
  });
});

describe("replaceGroupInTabOrder", () => {
  it("swaps a contiguous slice of ids", () => {
    expect(replaceGroupInTabOrder(["a", "b", "c", "d"], 1, 2, ["d", "c"])).toEqual([
      "a",
      "d",
      "c",
      "d",
    ]);
  });
});
