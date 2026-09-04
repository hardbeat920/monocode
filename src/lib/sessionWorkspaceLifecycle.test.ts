import { describe, expect, it } from "vitest";
import { leaf, newFileTab, newTab, type WorkspaceTab } from "./layout";
import { newSession, type Session } from "./session";
import {
  removeSessionFromWorkspace,
  type SessionWorkspaceRemoval,
} from "./sessionWorkspaceLifecycle";

function session(id: string, cwd = "/projects/monocode"): Session {
  return { ...newSession("cursor", cwd), id };
}

function tab(id: string, sessionId: string): WorkspaceTab {
  return { ...newTab(sessionId), id };
}

function remove(input: {
  tabs: WorkspaceTab[];
  sessions: Session[];
  sessionId: string;
  activeTabId: string;
  scope?: "project" | "workspace";
}): SessionWorkspaceRemoval {
  return removeSessionFromWorkspace({
    ...input,
    scope: input.scope ?? "workspace",
    createReplacement: (seed) => session("replacement", seed?.cwd),
  });
}

describe("removeSessionFromWorkspace", () => {
  it("closes the sole-session tab with its files instead of promoting them", () => {
    const file = newFileTab("/projects/monocode/README.md", "/projects/monocode");
    const closing = {
      ...tab("closing", "s1"),
      layout: {
        type: "split" as const,
        id: "split",
        dir: "right" as const,
        children: [leaf("s1"), leaf("editor")],
        sizes: [0.5, 0.5],
      },
      focusedId: "s1",
      editorPanes: [{ id: "editor", files: [file], activeFileId: file.id }],
    };
    const result = remove({
      tabs: [closing, tab("other", "s2")],
      sessions: [session("s1"), session("s2", "/projects/ruler")],
      sessionId: "s1",
      activeTabId: "closing",
    });

    expect(result.closedTabs.map((entry) => entry.id)).toEqual(["closing"]);
    expect(result.tabs.map((entry) => entry.id)).toEqual(["other"]);
    expect(result.sessions.map((entry) => entry.id)).toEqual(["s2"]);
    expect(result.activeTabId).toBe("other");
  });

  it("retains file panes and another conversation in a shared workspace", () => {
    const file = newFileTab("/projects/monocode/README.md", "/projects/monocode");
    const shared: WorkspaceTab = {
      ...tab("shared", "s1"),
      layout: {
        type: "split" as const,
        id: "outer",
        dir: "right" as const,
        children: [
          leaf("s1"),
          {
            type: "split",
            id: "inner",
            dir: "down",
            children: [leaf("s2"), leaf("editor")],
            sizes: [0.5, 0.5],
          },
        ],
        sizes: [0.5, 0.5],
      },
      focusedId: "s1",
      editorPanes: [{ id: "editor", files: [file], activeFileId: file.id }],
    };
    const result = remove({
      tabs: [shared],
      sessions: [session("s1"), session("s2")],
      sessionId: "s1",
      activeTabId: "shared",
    });

    expect(result.closedTabs).toEqual([]);
    expect(result.tabs).toHaveLength(1);
    expect(result.tabs[0]?.editorPanes).toEqual(shared.editorPanes);
    expect(result.tabs[0]?.layout).toEqual({
      type: "split",
      id: "inner",
      dir: "down",
      children: [leaf("s2"), leaf("editor")],
      sizes: [0.5, 0.5],
    });
    expect(result.sessions.map((entry) => entry.id)).toEqual(["s2"]);
  });

  it("closes session-scoped changes with the session while preserving other files", () => {
    const sessionChanges = {
      id: "changes",
      path: "/projects/monocode",
      cwd: "/projects/monocode",
      review: true,
      sessionChanges: { sessionId: "s1" },
    };
    const readme = newFileTab("/projects/monocode/README.md", "/projects/monocode");
    const shared: WorkspaceTab = {
      ...tab("shared", "s1"),
      layout: {
        type: "split" as const,
        id: "split",
        dir: "right" as const,
        children: [leaf("s1"), leaf("s2"), leaf("editor")],
        sizes: [1 / 3, 1 / 3, 1 / 3],
      },
      focusedId: "s1",
      editorPanes: [
        { id: "editor", files: [sessionChanges, readme], activeFileId: sessionChanges.id },
      ],
    };
    const result = remove({
      tabs: [shared],
      sessions: [session("s1"), session("s2")],
      sessionId: "s1",
      activeTabId: "shared",
    });

    expect(result.tabs[0]?.editorPanes[0]?.files).toEqual([readme]);
    expect(result.tabs[0]?.editorPanes[0]?.activeFileId).toBe(readme.id);
  });

  it("replaces the final project tab with a blank session", () => {
    const result = remove({
      tabs: [tab("only", "s1")],
      sessions: [session("s1")],
      sessionId: "s1",
      activeTabId: "only",
      scope: "project",
    });

    expect(result.closedTabs.map((entry) => entry.id)).toEqual(["only"]);
    expect(result.tabs[0]?.id).toBe("only");
    expect(result.tabs[0]?.focusedId).toBe("replacement");
    expect(result.sessions.map((entry) => entry.id)).toEqual(["replacement"]);
  });

  it("does not leave the project when closing its final tab in project scope", () => {
    const result = remove({
      tabs: [tab("ruler", "r1"), tab("monocode", "s1")],
      sessions: [session("r1", "/projects/ruler"), session("s1")],
      sessionId: "s1",
      activeTabId: "monocode",
      scope: "project",
    });

    expect(result.tabs.map((entry) => entry.id)).toEqual(["ruler", "monocode"]);
    expect(result.activeTabId).toBe("monocode");
    expect(result.tabs[1]?.focusedId).toBe("replacement");
  });
});
