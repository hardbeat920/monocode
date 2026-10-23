import { workspaceTabCwd } from "../../workspace/model/workspaceTabGroups";
import { describe, expect, it, vi } from "vitest";
import { leafIds, newFileTab, newTab, type WorkspaceTab } from "../../workspace/model/layout";
import type { Session } from "./session";
import { applyAddToChatRequest } from "./addChatToWorkspace";
import * as settings from "../../settings/model/settings";

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
  it.each(["cow", "worktree"] as const)(
    "keeps the selected worktree when new sessions default to %s",
    (mode) => {
      const preference = vi.spyOn(settings, "loadDefaultIsolationMode").mockReturnValue(mode);
      try {
        const tab = fileOnlyTab("feature", "/repo-worktrees/feature");
        tab.focusedId = "pane";
        tab.layout = { type: "leaf", id: "pane" };
        tab.editorPanes[0].files[0].projectCwd = "/repo";
        const result = applyAddToChatRequest({
          sessions: [],
          tabs: [tab],
          activeTabId: tab.id,
          projectCwd: "/repo",
          text: "Review this feature code",
        });
        const chat = newChat(result!);
        expect(chat.cwd).toBe("/repo");
        expect(chat.worktreeCwd).toBe("/repo-worktrees/feature");
        expect(chat.workspaceMode).toBeUndefined();
        expect(chat.worktreeBase).toBeUndefined();
        expect(chat.composerSeed).toContain("Review this feature code");
      } finally {
        preference.mockRestore();
      }
    },
  );

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

describe("add-to-chat from an existing copy-on-write workspace", () => {
  it.each([false, true])(
    "constructs a fresh copy session without mutating the source (no tabs: %s)",
    (empty) => {
      const source = {
        ...session("copy-source", "/repo"),
        cowId: "source",
        worktreeCwd: "/repo-cow/source",
      };
      const tab = fileOnlyTab("copy-file", source.worktreeCwd);
      const before = JSON.stringify({ source, tab });
      const result = applyAddToChatRequest({
        sessions: [source],
        tabs: empty ? [] : [tab],
        activeTabId: tab.id,
        projectCwd: source.worktreeCwd,
        text: "Review selected code",
      })!;
      const added = newChat(result);
      expect(added).toMatchObject({
        cwd: "/repo",
        workspaceMode: "cow",
        cowSourceCwd: source.worktreeCwd,
      });
      expect(added.worktreeCwd).toBeUndefined();
      expect(added.cowId).toBeUndefined();
      expect(result.tabs[0].focusedId).toBe(added.id);
      expect(workspaceTabCwd(result.tabs[0], result.sessions)).toBe("/repo");
      expect(JSON.stringify({ source, tab })).toBe(before);
    },
  );
});
