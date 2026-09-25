import { beforeEach, describe, expect, it } from "vitest";
import {
  clearProjectSidebarTab,
  loadProjectSidebarTab,
  rebaseProjectSidebarTab,
  saveProjectSidebarTab,
} from "./projectSidebarTab";

const KEY = "monocode.projectSidebarTabs.v1";

beforeEach(() => {
  const data = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => data.set(key, value),
      removeItem: (key: string) => data.delete(key),
    },
  });
});

describe("project Workspace tab", () => {
  it("remembers each project's tab across path spelling changes", () => {
    saveProjectSidebarTab("/work/one/", "files");
    saveProjectSidebarTab("/work/two", "changes");

    expect(loadProjectSidebarTab("/work/one")).toBe("files");
    expect(loadProjectSidebarTab("/work/two/")).toBe("changes");
    expect(loadProjectSidebarTab("/work/three")).toBe("sessions");
  });

  it("ignores invalid saved tabs and never saves Inbox as a Workspace tab", () => {
    localStorage.setItem(KEY, JSON.stringify({ "/work/one": "unknown" }));
    expect(loadProjectSidebarTab("/work/one")).toBe("sessions");

    saveProjectSidebarTab("/work/one", "inbox");
    expect(loadProjectSidebarTab("/work/one")).toBe("sessions");
  });

  it("uses the first visible tab in the global order for a new project", () => {
    localStorage.setItem(
      "monocode.sidebarTabOrder",
      JSON.stringify(["inbox", "files", "sessions", "changes"]),
    );
    expect(loadProjectSidebarTab("/work/new")).toBe("files");
  });

  it("follows a project rename and clears a deleted project's choice", () => {
    saveProjectSidebarTab("/work/one", "files");
    rebaseProjectSidebarTab("/work/one", "/work/renamed");

    expect(loadProjectSidebarTab("/work/renamed")).toBe("files");
    expect(loadProjectSidebarTab("/work/one")).toBe("sessions");

    clearProjectSidebarTab("/work/renamed");
    expect(loadProjectSidebarTab("/work/renamed")).toBe("sessions");
  });
});
