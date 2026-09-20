import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  archiveProject,
  forgetProject,
  loadArchivedProjects,
  loadPinnedProjects,
  loadProjectRailOrder,
  loadRecents,
  looksLikeProject,
  projectRailItems,
  projectRailSections,
  rememberProject,
  savePinnedProjects,
  saveProjectRailOrder,
  syncProjectRailOrder,
} from "./recents";

function mockLocalStorage() {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => {
      data.clear();
    },
    key: (index: number) => [...data.keys()][index] ?? null,
    get length() {
      return data.size;
    },
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
  });
}

describe("looksLikeProject", () => {
  it("rejects the home directory so it is never indexed", () => {
    // Home arrives expanded from `default_cwd`. Walking it reaches
    // ~/Library, which makes macOS prompt for access to other apps' data.
    expect(looksLikeProject("/Users/me")).toBe(false);
    expect(looksLikeProject("/Users/me/")).toBe(false);
    expect(looksLikeProject("/home/me")).toBe(false);
    expect(looksLikeProject("C:/Users/me")).toBe(false);
    expect(looksLikeProject("C:\\Users\\me")).toBe(false);
    expect(looksLikeProject("~")).toBe(false);
  });

  it("rejects system roots and app bundles", () => {
    expect(looksLikeProject("/")).toBe(false);
    expect(looksLikeProject("")).toBe(false);
    expect(looksLikeProject("C:/")).toBe(false);
    expect(looksLikeProject("C:")).toBe(false);
    expect(looksLikeProject("/Applications/Some.app/Contents")).toBe(false);
  });

  it("accepts real projects, including ones directly under home", () => {
    expect(looksLikeProject("/Users/me/code/app")).toBe(true);
    expect(looksLikeProject("/Users/me/Desktop")).toBe(true);
    expect(looksLikeProject("/tmp/scratch")).toBe(true);
    expect(looksLikeProject("C:/Users/me/code/app")).toBe(true);
  });
});

describe("projectRailSections", () => {
  it("keeps saved order and does not move the current project first", () => {
    const recents = [
      { path: "/tmp/older", openedAt: 1 },
      { path: "/tmp/current", openedAt: 2 },
    ];
    const { pinned, projects } = projectRailSections(
      recents,
      "/tmp/current/",
      ["/tmp/older", "/tmp/current"],
      [],
    );
    expect([...pinned, ...projects].map((item) => item.path)).toEqual([
      "/tmp/older",
      "/tmp/current",
    ]);
  });

  it("places pinned projects before unpinned ones", () => {
    const recents = [
      { path: "/tmp/a", openedAt: 1 },
      { path: "/tmp/b", openedAt: 2 },
      { path: "/tmp/c", openedAt: 3 },
    ];
    const { pinned, projects } = projectRailSections(
      recents,
      "/tmp/a",
      ["/tmp/a", "/tmp/b", "/tmp/c"],
      ["/tmp/b"],
    );
    expect(pinned.map((item) => item.path)).toEqual(["/tmp/b"]);
    expect(projects.map((item) => item.path)).toEqual(["/tmp/a", "/tmp/c"]);
  });

  it("appends new projects without reordering existing entries", () => {
    const recents = [
      { path: "/tmp/older", openedAt: 1 },
      { path: "/tmp/new", openedAt: 3 },
    ];
    const projects = new Map([
      ["/tmp/older", { path: "/tmp/older", openedAt: 1 }],
      ["/tmp/new", { path: "/tmp/new", openedAt: 3 }],
    ]);
    expect(syncProjectRailOrder(["/tmp/older"], projects)).toEqual([
      "/tmp/older",
      "/tmp/new",
    ]);
  });
});

describe("projectRailItems", () => {
  it("ignores home as a current folder", () => {
    expect(
      projectRailItems([{ path: "/tmp/app", openedAt: 1 }], "/Users/me").map(
        (item) => item.path,
      ),
    ).toEqual(["/tmp/app"]);
  });
});

describe("forgetProject", () => {
  beforeEach(() => {
    mockLocalStorage();
  });

  afterEach(() => {
    mockLocalStorage();
  });

  it("drops the recent entry, rail order slot, and pin", () => {
    rememberProject("/tmp/keep");
    rememberProject("/tmp/gone");
    saveProjectRailOrder(["/tmp/keep", "/tmp/gone"]);
    savePinnedProjects(["/tmp/gone"]);

    expect(forgetProject("/tmp/gone").map((item) => item.path)).toEqual([
      "/tmp/keep",
    ]);
    expect(loadRecents().map((item) => item.path)).toEqual(["/tmp/keep"]);
    expect(loadProjectRailOrder()).toEqual(["/tmp/keep"]);
    expect(loadPinnedProjects()).toEqual([]);
  });

  it("treats differently-cased Windows paths as one project", () => {
    rememberProject("C:/Users/me/Code/App");
    rememberProject("c:/users/ME/code/app");
    expect(loadRecents().map((item) => item.path)).toEqual([
      "c:/users/ME/code/app",
    ]);
  });
});

describe("archiveProject", () => {
  beforeEach(() => {
    mockLocalStorage();
  });

  afterEach(() => {
    mockLocalStorage();
  });

  it("files the project in the archive and takes it off the rail", () => {
    rememberProject("/tmp/keep");
    rememberProject("/tmp/gone");
    savePinnedProjects(["/tmp/gone"]);

    expect(archiveProject("/tmp/gone").map((item) => item.path)).toEqual([
      "/tmp/keep",
    ]);
    expect(loadArchivedProjects().map((item) => item.path)).toEqual([
      "/tmp/gone",
    ]);
    expect(loadPinnedProjects()).toEqual([]);
    expect(loadRecents().map((item) => item.path)).toEqual(["/tmp/keep"]);
  });

  it("opening a project again restores it from the archive", () => {
    rememberProject("/tmp/gone");
    archiveProject("/tmp/gone");
    expect(loadArchivedProjects()).toHaveLength(1);

    rememberProject("/tmp/gone");
    expect(loadArchivedProjects()).toEqual([]);
    expect(loadRecents().map((item) => item.path)).toEqual(["/tmp/gone"]);
  });

  it("delete drops an archived project instead of restoring it", () => {
    rememberProject("/tmp/gone");
    archiveProject("/tmp/gone");
    forgetProject("/tmp/gone");
    expect(loadArchivedProjects()).toEqual([]);
    expect(loadRecents()).toEqual([]);
  });
});
