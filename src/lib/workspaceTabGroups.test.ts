import { describe, expect, it } from "vitest";
import { newTab, type WorkspaceTab } from "./layout";
import type { Session } from "./session";
import {
  filterTabsForProject,
  findTabForProject,
  planWorkspaceTabCloseTarget,
  replaceGroupInTabOrder,
  workspaceTabProject,
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

describe("planWorkspaceTabCloseTarget", () => {
  const sessions = [
    session("m1", "/projects/monocode"),
    session("r1", "/projects/ruler"),
    session("m2", "/projects/monocode"),
  ];
  const tabs = [tab("tm1", "m1"), tab("tr1", "r1"), tab("tm2", "m2")];

  it("uses the global neighbor in workspace scope", () => {
    expect(
      planWorkspaceTabCloseTarget({
        tabs,
        sessions,
        closingTabId: "tm2",
        scope: "workspace",
      }),
    ).toBe("tr1");
  });

  it("prefers the previous same-project tab in project scope", () => {
    expect(
      planWorkspaceTabCloseTarget({
        tabs,
        sessions,
        closingTabId: "tm2",
        scope: "project",
      }),
    ).toBe("tm1");
  });

  it("uses the next same-project tab when none exists to the left", () => {
    expect(
      planWorkspaceTabCloseTarget({
        tabs,
        sessions,
        closingTabId: "tm1",
        scope: "project",
      }),
    ).toBe("tm2");
  });

  it("falls back to the global neighbor when the project has no other tab", () => {
    expect(
      planWorkspaceTabCloseTarget({
        tabs: tabs.slice(0, 2),
        sessions,
        closingTabId: "tm1",
        scope: "project",
      }),
    ).toBe("tr1");
  });

  it("uses the global neighbor for a projectless tab", () => {
    const projectlessSessions = [
      ...sessions,
      session("blank1", "~"),
      session("blank2", "~"),
    ];
    expect(
      planWorkspaceTabCloseTarget({
        tabs: [tab("projectless", "blank1"), tabs[1]],
        sessions: projectlessSessions,
        closingTabId: "projectless",
        scope: "project",
      }),
    ).toBe("tr1");
    expect(
      planWorkspaceTabCloseTarget({
        tabs: [tab("blank1", "blank1"), tabs[1], tab("blank2", "blank2")],
        sessions: projectlessSessions,
        closingTabId: "blank2",
        scope: "project",
      }),
    ).toBe("tr1");
  });

  it("returns undefined for the sole tab or an unknown tab", () => {
    expect(
      planWorkspaceTabCloseTarget({
        tabs: [tabs[0]],
        sessions,
        closingTabId: "tm1",
        scope: "workspace",
      }),
    ).toBeUndefined();
    expect(
      planWorkspaceTabCloseTarget({
        tabs,
        sessions,
        closingTabId: "missing",
        scope: "project",
      }),
    ).toBeUndefined();
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
