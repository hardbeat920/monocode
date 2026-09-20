import { describe, expect, it } from "vitest";
import { leafIds, newFileTab, newTab, type WorkspaceTab } from "../../workspace/model/layout";
import type { Session } from "./session";
import { applyAddToChatRequest } from "./addChatToWorkspace";

function session(id: string, cwd: string, overrides: Partial<Session> = {}): Session {
  return {
    id,
    cwd,
    harness: "cursor",
    title: "",
    blocks: [],
    busy: false,
    model: "",
    ...overrides,
  };
}

function fileOnlyTab(id: string, cwd: string): WorkspaceTab {
  const file = newFileTab(`${cwd}/readme.md`, cwd);
  return {
    ...newTab("unused"),
    id,
    editorPanes: [{ id: "pane", files: [file], activeFileId: file.id }],
  };
}

function newChat(result: NonNullable<ReturnType<typeof applyAddToChatRequest>>): Session {
  return result.sessions.find((s) => s.id === result.sessionId)!;
}

describe("applyAddToChatRequest: zero-tab fallback", () => {
  it("creates exactly one seeded session hosted by exactly one pane", () => {
    const donor = session("s1", "/other/project", {
      harness: "claude",
      model: "claude:opus-5",
      runtimeMode: "auto",
    });
    const result = applyAddToChatRequest({
      sessions: [donor],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(result).not.toBeNull();
    expect(result!.sessions).toHaveLength(2);
    expect(result!.sessionId).toBe(newChat(result!).id);
    // No duplicate split: the fallback tab hosts the new session exactly once.
    expect(leafIds(result!.tabs[0].layout)).toEqual([result!.sessionId]);
    expect(result!.tabs[0].focusedId).toBe(result!.sessionId);
    expect(result!.activeTabId).toBe(result!.tabs[0].id);
  });

  it("seeds the composer with the quoted text", () => {
    const result = applyAddToChatRequest({
      sessions: [],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(newChat(result!).composerSeed).toContain("selected code");
    expect(newChat(result!).composerSeed).toMatch(/^>/);
  });

  it("keeps the donor session's harness, model and runtime mode", () => {
    const donor = session("s1", "/other/project", {
      harness: "claude",
      model: "claude:opus-5",
      runtimeMode: "auto",
    });
    const result = applyAddToChatRequest({
      sessions: [donor],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(newChat(result!).harness).toBe("claude");
    expect(newChat(result!).model).toBe("claude:opus-5");
    expect(newChat(result!).runtimeMode).toBe("auto");
  });

  it("uses the project cwd, never another project's session cwd", () => {
    const donor = session("s1", "/other/project");
    const result = applyAddToChatRequest({
      sessions: [donor],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(newChat(result!).cwd).toBe("/current/project");
  });

  it("donates settings from the last known session, not the first", () => {
    const first = session("s1", "/other/project", {
      harness: "codex",
      model: "codex:gpt-5",
      runtimeMode: "full-access",
    });
    const last = session("s2", "/other/project", {
      harness: "claude",
      model: "claude:opus-5",
      runtimeMode: "auto",
    });
    const result = applyAddToChatRequest({
      sessions: [first, last],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(newChat(result!).harness).toBe("claude");
    expect(newChat(result!).model).toBe("claude:opus-5");
    expect(newChat(result!).runtimeMode).toBe("auto");
  });

  it("falls back to Claude defaults with an empty workspace", () => {
    const result = applyAddToChatRequest({
      sessions: [],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(newChat(result!).harness).toBe("claude");
    expect(newChat(result!).cwd).toBe("/current/project");
  });

  it("returns null for whitespace-only text", () => {
    expect(
      applyAddToChatRequest({
        sessions: [],
        tabs: [],
        projectCwd: "/current/project",
        text: "   \n  ",
      }),
    ).toBeNull();
  });
});

describe("applyAddToChatRequest: file-only tab", () => {
  it("splits the new chat beside the file pane", () => {
    const tab = fileOnlyTab("tab1", "/current/project");
    const result = applyAddToChatRequest({
      sessions: [],
      tabs: [tab],
      activeTabId: "tab1",
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(result).not.toBeNull();
    expect(leafIds(result!.tabs[0].layout)).toEqual([
      "unused",
      result!.sessionId,
    ]);
    expect(result!.tabs[0].focusedId).toBe(result!.sessionId);
    expect(result!.sessions).toHaveLength(1);
  });

  it("bails when the target tab already shows a mounted session", () => {
    const mounted = session("s1", "/current/project");
    const tab = newTab("s1");
    const result = applyAddToChatRequest({
      sessions: [mounted],
      tabs: [tab],
      activeTabId: tab.id,
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(result).toBeNull();
  });
});
