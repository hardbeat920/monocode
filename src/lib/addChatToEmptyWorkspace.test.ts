import { describe, expect, it } from "vitest";
import { leafIds } from "./layout";
import type { Session } from "./session";
import { addChatToEmptyWorkspace } from "./addChatToEmptyWorkspace";

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

describe("addChatToEmptyWorkspace", () => {
  it("creates exactly one seeded session and one tab using it directly", () => {
    const donor = session("s1", "/other/project", {
      harness: "claude",
      model: "claude:opus-5",
      runtimeMode: "auto",
    });
    const result = addChatToEmptyWorkspace({
      sessions: [donor],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(result).not.toBeNull();
    expect(result!.sessions).toHaveLength(2);
    expect(result!.tabs).toHaveLength(1);
    expect(result!.session).toBe(result!.sessions[1]);
    expect(result!.tab).toBe(result!.tabs[0]);
    expect(leafIds(result!.tab.layout)).toEqual([result!.session.id]);
    expect(result!.tab.focusedId).toBe(result!.session.id);
  });

  it("seeds the composer with the quoted text", () => {
    const result = addChatToEmptyWorkspace({
      sessions: [],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(result!.session.composerSeed).toContain("selected code");
    expect(result!.session.composerSeed).toMatch(/^>/);
  });

  it("keeps the donor session's harness, model and runtime mode", () => {
    const donor = session("s1", "/other/project", {
      harness: "claude",
      model: "claude:opus-5",
      runtimeMode: "auto",
    });
    const result = addChatToEmptyWorkspace({
      sessions: [donor],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(result!.session.harness).toBe("claude");
    expect(result!.session.model).toBe("claude:opus-5");
    expect(result!.session.runtimeMode).toBe("auto");
  });

  it("uses the project cwd, never another project's session cwd", () => {
    const donor = session("s1", "/other/project");
    const result = addChatToEmptyWorkspace({
      sessions: [donor],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(result!.session.cwd).toBe("/current/project");
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
    const result = addChatToEmptyWorkspace({
      sessions: [first, last],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(result!.session.harness).toBe("claude");
    expect(result!.session.model).toBe("claude:opus-5");
    expect(result!.session.runtimeMode).toBe("auto");
  });

  it("falls back to Claude defaults with an empty workspace", () => {
    const result = addChatToEmptyWorkspace({
      sessions: [],
      tabs: [],
      projectCwd: "/current/project",
      text: "selected code",
    });

    expect(result!.session.harness).toBe("claude");
    expect(result!.session.cwd).toBe("/current/project");
  });

  it("returns null for whitespace-only text", () => {
    expect(
      addChatToEmptyWorkspace({
        sessions: [],
        tabs: [],
        projectCwd: "/current/project",
        text: "   \n  ",
      }),
    ).toBeNull();
  });
});
