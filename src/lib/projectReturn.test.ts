import { describe, expect, it } from "vitest";
import { newFileTab, newTab, placePane, type WorkspaceTab } from "./layout";
import { newSession, type Session } from "./session";
import { planProjectReturn, reconcileProjectReturn } from "./projectReturn";
import {
  emptyTabVisitHistory,
  recordTabVisit,
  tabVisitBack,
} from "./tabVisitHistory";
import { removeSessionFromWorkspace } from "./sessionWorkspaceLifecycle";
import { applyPlaceSessionOnPane } from "./workspaceTabGroups";

function chat(id: string, cwd: string): Session {
  return {
    ...newSession("cursor", cwd),
    id,
    blocks: [{ id: `${id}-user`, role: "user", text: id }],
  };
}

function workspace() {
  const sessions = [
    chat("a1", "/alpha"),
    chat("a2", "/alpha"),
    chat("b1", "/beta"),
    chat("b2", "/beta"),
  ];
  const tabs = sessions.map((session) => ({
    ...newTab(session.id),
    id: `tab-${session.id}`,
  }));
  return {
    sessions,
    tabs,
    activeTabId: "tab-b2",
    memory: new Map<string, string>(),
  };
}

describe("project return memory", () => {
  it("records visits independently and changes on explicit selection or Back", () => {
    const state = workspace();
    const alpha = reconcileProjectReturn({ ...state, activeTabId: "tab-a2" });
    const beta = reconcileProjectReturn({ ...state, memory: alpha });
    expect([...beta]).toEqual([
      ["/alpha", "tab-a2"],
      ["/beta", "tab-b2"],
    ]);
    const changed = reconcileProjectReturn({
      ...state,
      memory: beta,
      activeTabId: "tab-a1",
    });
    expect(changed.get("/alpha")).toBe("tab-a1");
    const history = recordTabVisit(emptyTabVisitHistory("tab-a2"), "tab-b2");
    const back = tabVisitBack(history);
    expect(back).not.toBeNull();
    const returned = reconcileProjectReturn({
      ...state,
      memory: changed,
      activeTabId: back?.current ?? "",
    });
    expect(returned.get("/alpha")).toBe("tab-a2");
    expect(returned.get("/beta")).toBe("tab-b2");
    expect(state.memory.size).toBe(0);
    expect(alpha.size).toBe(1);
  });

  it("does not change identity for repeated observation or streaming updates", () => {
    const state = workspace();
    const memory = reconcileProjectReturn(state);
    state.sessions[0].blocks.push({
      id: "response",
      role: "assistant",
      text: "working",
    });
    state.sessions[0].busy = true;
    expect(reconcileProjectReturn({ ...state, memory })).toBe(memory);
  });

  it("prunes removed and retargeted tabs without changing the input map", () => {
    const state = workspace();
    const memory = new Map([
      ["/alpha", "tab-a2"],
      ["/gone", "missing"],
      ["/beta", "tab-b2"],
    ]);
    state.sessions[1].cwd = "/gamma";
    expect([...reconcileProjectReturn({ ...state, memory })]).toEqual([
      ["/beta", "tab-b2"],
    ]);
    expect(memory.size).toBe(3);
  });

  it("records editor-only tabs and keeps the selected file pane", () => {
    const file = newFileTab("/gamma/readme.md", "/gamma");
    const tab = {
      ...newTab("editor"),
      id: "files",
      editorPanes: [{ id: "editor", files: [file], activeFileId: file.id }],
    };
    const state = workspace();
    const tabs = [...state.tabs, tab];
    const memory = reconcileProjectReturn({
      ...state,
      tabs,
      activeTabId: tab.id,
    });
    expect(
      planProjectReturn({ ...state, tabs, memory, projectPath: "/gamma" }),
    ).toEqual({ action: "activate", tabId: tab.id });
    expect(tab.focusedId).toBe("editor");
  });

  it("records canonical Windows keys and does not combine matching display names", () => {
    const state = workspace();
    state.sessions[1].cwd = "C:/Work/Alpha/";
    const memory = reconcileProjectReturn({ ...state, activeTabId: "tab-a2" });
    expect(memory.get("c:/work/alpha")).toBe("tab-a2");
    expect(
      planProjectReturn({ ...state, memory, projectPath: "c:\\work\\ALPHA" }),
    ).toEqual({ action: "activate", tabId: "tab-a2" });
    expect(
      planProjectReturn({ ...state, memory, projectPath: "/elsewhere/Alpha" }),
    ).toEqual({ action: "create" });
  });

  it("validates the result of actual session removal and preserves close replacement", () => {
    const state = workspace();
    const memory = new Map([
      ["/alpha", "tab-a2"],
      ["/beta", "tab-b2"],
    ]);
    const removed = removeSessionFromWorkspace({
      ...state,
      sessionId: "a2",
      scope: "project",
      createReplacement: (seed) => chat("replacement", seed?.cwd ?? "/alpha"),
    });
    const next = reconcileProjectReturn({ ...removed, memory });
    expect(
      planProjectReturn({ ...removed, memory: next, projectPath: "/alpha" }),
    ).toEqual({ action: "activate", tabId: "tab-a1" });
    const activeRemoved = removeSessionFromWorkspace({
      ...state,
      activeTabId: "tab-a2",
      sessionId: "a2",
      scope: "project",
      createReplacement: (seed) => chat("replacement", seed?.cwd ?? "/alpha"),
    });
    expect(
      reconcileProjectReturn({ ...activeRemoved, memory }).get("/alpha"),
    ).toBe(activeRemoved.activeTabId);
  });

  it("returns to the focused session after actual pane placement", () => {
    const state = workspace();
    const placed = applyPlaceSessionOnPane({
      tabs: state.tabs,
      sessions: state.sessions,
      sessionId: "a2",
      targetId: "a1",
      edge: "right",
      replaceTarget: false,
      scope: "workspace",
      createReplacement: (seed) => chat("replacement", seed?.cwd ?? "/alpha"),
    });
    expect(placed).not.toBeNull();
    if (!placed) throw new Error("Expected placed workspace");
    const memory = reconcileProjectReturn({
      ...placed,
      memory: new Map([["/alpha", "tab-a2"]]),
    });
    const beta = reconcileProjectReturn({
      ...placed,
      memory,
      activeTabId: "tab-b2",
    });
    const decision = planProjectReturn({
      ...placed,
      memory: beta,
      activeTabId: "tab-b2",
      projectPath: "/alpha",
    });
    expect(decision).toEqual({ action: "activate", tabId: "tab-a1" });
    expect(placed.tabs.find((tab) => tab.id === "tab-a1")?.focusedId).toBe(
      "a2",
    );
  });

  it("drops a removed project's choice rather than reopening it", () => {
    const state = workspace();
    const memory = new Map([["/alpha", "tab-a2"]]);
    const remaining = {
      ...state,
      tabs: state.tabs.filter((tab) => tab.id.startsWith("tab-b")),
    };
    const next = reconcileProjectReturn({ ...remaining, memory });
    expect(next.has("/alpha")).toBe(false);
    expect(
      planProjectReturn({ ...remaining, memory: next, projectPath: "/alpha" }),
    ).toEqual({ action: "create" });
  });

  it("does not record a missing active tab or a projectless tab", () => {
    const state = workspace();
    expect(reconcileProjectReturn({ ...state, activeTabId: "missing" })).toBe(
      state.memory,
    );
    state.sessions[3].cwd = "~";
    expect(reconcileProjectReturn(state)).toBe(state.memory);
  });
});

describe("project selection", () => {
  it.each(["missing", "tab-b1"])("rejects invalid Alpha target %s", (tabId) => {
    const state = workspace();
    state.memory.set("/alpha", tabId);
    expect(planProjectReturn({ ...state, projectPath: "/alpha" })).toEqual({
      action: "activate",
      tabId: "tab-a1",
    });
  });

  it("keeps the focused project", () => {
    expect(
      planProjectReturn({ ...workspace(), projectPath: "/beta/" }),
    ).toEqual({ action: "keep" });
  });

  it("selects the first destination tab without a remembered choice", () => {
    expect(
      planProjectReturn({ ...workspace(), projectPath: "/alpha" }),
    ).toEqual({ action: "activate", tabId: "tab-a1" });
  });

  it("prefers an existing destination over a blank source", () => {
    const state = workspace();
    state.sessions[3].blocks = [];
    expect(planProjectReturn({ ...state, projectPath: "/alpha" })).toEqual({
      action: "activate",
      tabId: "tab-a1",
    });
  });

  it("returns to A2 and B2 independently of tab order", () => {
    const state = workspace();
    state.memory = new Map([
      ["/alpha", "tab-a2"],
      ["/beta", "tab-b2"],
    ]);
    expect(planProjectReturn({ ...state, projectPath: "/alpha" })).toEqual({
      action: "activate",
      tabId: "tab-a2",
    });
    expect(
      planProjectReturn({
        ...state,
        activeTabId: "tab-a2",
        projectPath: "/beta",
      }),
    ).toEqual({ action: "activate", tabId: "tab-b2" });
    expect(
      planProjectReturn({
        ...state,
        tabs: [...state.tabs].reverse(),
        projectPath: "/alpha",
      }),
    ).toEqual({ action: "activate", tabId: "tab-a2" });
  });

  it("reuses a blank only when the destination has no open tab", () => {
    const state = workspace();
    state.sessions[3].blocks = [];
    expect(planProjectReturn({ ...state, projectPath: "/gamma" })).toEqual({
      action: "reuse-blank",
      sessionId: "b2",
    });
  });

  it("never reuses a busy session even when it has no user blocks", () => {
    const state = workspace();
    state.sessions[3].blocks = [];
    state.sessions[3].busy = true;
    expect(planProjectReturn({ ...state, projectPath: "/gamma" })).toEqual({
      action: "create",
    });
  });

  it("creates a session when no destination exists and the current chat is not blank", () => {
    expect(
      planProjectReturn({ ...workspace(), projectPath: "/gamma" }),
    ).toEqual({ action: "create" });
  });

  it("keeps a focused Beta session in an Alpha-first split", () => {
    const state = workspace();
    const first = state.tabs[0];
    state.tabs[0] = {
      ...first,
      layout: placePane(first.layout, "b2", "a1", "right"),
      focusedId: "b2",
    };
    state.tabs = state.tabs.filter((tab) => tab.id !== "tab-b2");
    expect(
      planProjectReturn({
        ...state,
        activeTabId: first.id,
        projectPath: "/beta",
      }),
    ).toEqual({ action: "keep" });
  });

  it("keeps a focused file in the selected project", () => {
    const state = workspace();
    const file = newFileTab("/gamma/readme.md", "/gamma");
    const tab: WorkspaceTab = {
      ...newTab("editor"),
      id: "files",
      editorPanes: [{ id: "editor", files: [file], activeFileId: file.id }],
    };
    expect(
      planProjectReturn({
        ...state,
        tabs: [...state.tabs, tab],
        activeTabId: tab.id,
        projectPath: "/gamma",
      }),
    ).toEqual({ action: "keep" });
  });
});
