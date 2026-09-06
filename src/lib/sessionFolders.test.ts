import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { SessionSummary } from "./sessionStore";
import {
  addSessionToFolder,
  applySessionListDrop,
  buildSessionList,
  createFolderWithSessions,
  dissolveFolder,
  folderAccent,
  folderContaining,
  folderShellFill,
  loadSessionFolders,
  mergeFolderSessionSummaries,
  pruneSessionFolders,
  removeSessionFromFolder,
  renameFolder,
  saveSessionFolders,
  sessionListNavigationIds,
  setFolderCollapsed,
  setFolderColor,
  setFolderCustomColor,
  reorderSessionFolders,
  ungroupedSessions,
  uniqueFolderName,
  type SessionFolder,
} from "./sessionFolders";

function summary(
  id: string,
  overrides: Partial<SessionSummary> = {},
): SessionSummary {
  return {
    id,
    cwd: "/tmp/project",
    harness: "cursor",
    model: "gpt-5",
    runtimeMode: "supervised",
    title: `cursor · ${id}`,
    createdAt: 1,
    updatedAt: 1,
    additions: 0,
    deletions: 0,
    ...overrides,
  };
}

function folder(
  id: string,
  sessionIds: string[],
  overrides: Partial<SessionFolder> = {},
): SessionFolder {
  return {
    id,
    name: id,
    sessionIds,
    collapsed: false,
    ...overrides,
  };
}

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

describe("uniqueFolderName", () => {
  it("uses New folder, then numbers", () => {
    expect(uniqueFolderName([])).toBe("New folder");
    expect(uniqueFolderName([folder("a", ["s"], { name: "New folder" })])).toBe(
      "New folder 2",
    );
    expect(
      uniqueFolderName([
        folder("a", ["s1"], { name: "New folder" }),
        folder("b", ["s2"], { name: "New folder 2" }),
      ]),
    ).toBe("New folder 3");
  });
});

describe("buildSessionList", () => {
  it("places folders above pinned ungrouped sessions", () => {
    const sessions = [
      summary("pin", { pinned: true, updatedAt: 1 }),
      summary("new", { updatedAt: 9 }),
      summary("in-folder", { updatedAt: 5 }),
    ];
    const folders = [folder("work", ["in-folder"], { name: "Work" })];
    const entries = buildSessionList(
      sessions,
      folders,
      ungroupedSessions(sessions, folders),
    );
    expect(
      entries.map((entry) =>
        entry.kind === "folder"
          ? entry.folder.name
          : entry.kind === "session"
            ? entry.session.id
            : "divider",
      ),
    ).toEqual(["Work", "pin", "divider", "new"]);
  });

  it("hides folders whose members are not in the visible set", () => {
    const sessions = [summary("a")];
    const folders = [folder("hidden", ["gone"])];
    expect(buildSessionList(sessions, folders, sessions)).toEqual([
      { kind: "session", session: sessions[0] },
    ]);
  });

  it("surfaces an open folder member that is not in history yet", () => {
    const visible = [summary("a")];
    const extra = summary("blank");
    const folders = [folder("work", ["a", "blank"])];
    const merged = mergeFolderSessionSummaries(visible, [extra], folders);
    expect(merged.map((session) => session.id)).toEqual(["a", "blank"]);
    expect(mergeFolderSessionSummaries(visible, [extra], [])).toBe(visible);
  });

  it("sorts members inside a folder by pin then recency", () => {
    const sessions = [
      summary("old", { updatedAt: 1 }),
      summary("pinned", { pinned: true, updatedAt: 2 }),
      summary("new", { updatedAt: 9 }),
    ];
    const folders = [folder("g", ["old", "new", "pinned"])];
    const entries = buildSessionList(sessions, folders, []);
    expect(entries[0]?.kind).toBe("folder");
    if (entries[0]?.kind !== "folder") return;
    expect(entries[0].sessions.map((row) => row.id)).toEqual([
      "pinned",
      "new",
      "old",
    ]);
  });

  it("keeps the pin split among ungrouped sessions when there are no folders", () => {
    const sessions = [
      summary("pin", { pinned: true }),
      summary("rest"),
    ];
    expect(
      buildSessionList(sessions, [], sessions).map((entry) =>
        entry.kind === "session" ? entry.session.id : entry.kind,
      ),
    ).toEqual(["pin", "divider", "rest"]);
  });

  it("exposes the full visible navigation order without pagination", () => {
    const sessions = [
      summary("folder-a"),
      summary("folder-b"),
      summary("loose"),
    ];
    const folders = [
      folder("work", ["folder-a", "folder-b"], { collapsed: true }),
    ];
    const entries = buildSessionList(
      sessions,
      folders,
      ungroupedSessions(sessions, folders),
    );
    expect(sessionListNavigationIds(entries, false)).toEqual(["loose"]);
    expect(sessionListNavigationIds(entries, true)).toEqual([
      "folder-a",
      "folder-b",
      "loose",
    ]);
  });
});

describe("folder mutations", () => {
  it("creates a folder at the top and pulls members out of other folders", () => {
    const existing = [folder("old", ["a", "c"], { name: "Old" })];
    const { folders, id } = createFolderWithSessions(existing, ["a", "b"]);
    expect(id).toBeTruthy();
    expect(folders[0]).toMatchObject({
      id,
      name: "New folder",
      sessionIds: ["a", "b"],
      collapsed: false,
    });
    expect(folders[1]).toMatchObject({ id: "old", sessionIds: ["c"] });
  });

  it("adds a session to a folder and drops an emptied source folder", () => {
    const folders = [
      folder("src", ["a"]),
      folder("dst", ["b"]),
    ];
    const next = addSessionToFolder(folders, "dst", "a");
    expect(next.map((entry) => entry.id)).toEqual(["dst"]);
    expect(next[0]?.sessionIds).toEqual(["b", "a"]);
  });

  it("is a no-op when adding a session already in that folder", () => {
    const folders = [folder("g", ["a"])];
    expect(addSessionToFolder(folders, "g", "a")).toBe(folders);
  });

  it("removes a session and deletes the folder when it would be empty", () => {
    const folders = [folder("g", ["a", "b"])];
    const next = removeSessionFromFolder(folders, "a");
    expect(next[0]?.sessionIds).toEqual(["b"]);
    expect(removeSessionFromFolder(next, "b")).toEqual([]);
  });

  it("dissolves a folder without touching the others", () => {
    const folders = [folder("a", ["s1"]), folder("b", ["s2"])];
    expect(dissolveFolder(folders, "a").map((entry) => entry.id)).toEqual(["b"]);
  });

  it("renames and ignores a blank name", () => {
    const folders = [folder("g", ["a"], { name: "Work" })];
    expect(renameFolder(folders, "g", "  Sprint  ")[0]?.name).toBe("Sprint");
    expect(renameFolder(folders, "g", "   ")).toBe(folders);
  });

  it("toggles collapsed without rewriting unchanged folders", () => {
    const folders = [folder("g", ["a"], { collapsed: false })];
    expect(setFolderCollapsed(folders, "g", false)).toBe(folders);
    expect(setFolderCollapsed(folders, "g", true)[0]?.collapsed).toBe(true);
  });

  it("stores a palette color and clears it back to the default wash", () => {
    const folders = [folder("g", ["a"])];
    const tinted = setFolderColor(folders, "g", 2);
    expect(tinted[0]?.colorIndex).toBe(2);
    expect(setFolderColor(tinted, "g", 2)).toBe(tinted);
    expect(setFolderColor(tinted, "g", 0)[0]?.colorIndex).toBeUndefined();
    expect(setFolderColor(tinted, "g", null)[0]?.colorIndex).toBeUndefined();
    expect(folderAccent(2)).toBeTruthy();
    expect(folderAccent(0)).toBeUndefined();
    expect(folderShellFill(2)).toMatch(/^color-mix\(/);
    expect(folderShellFill(undefined)).toBeUndefined();
  });

  it("stores a custom hex and prefers it over a palette index", () => {
    const folders = [folder("g", ["a"], { colorIndex: 2 })];
    const custom = setFolderCustomColor(folders, "g", "#3B82F6");
    expect(custom[0]?.customColor).toBe("#3b82f6");
    expect(custom[0]?.colorIndex).toBeUndefined();
    expect(setFolderCustomColor(custom, "g", "#3b82f6")).toBe(custom);
    expect(setFolderCustomColor(custom, "g", "not-a-color")).toBe(custom);
    expect(setFolderCustomColor(custom, "g", null)[0]?.customColor).toBeUndefined();
    expect(folderAccent(2, "#3b82f6")).toBe("#3b82f6");
    expect(folderShellFill(undefined, "#3b82f6")).toMatch(
      /color-mix\(in srgb, #3b82f6 18%/,
    );
    const preset = setFolderColor(custom, "g", 3);
    expect(preset[0]?.colorIndex).toBe(3);
    expect(preset[0]?.customColor).toBeUndefined();
    expect(setFolderColor(custom, "g", null)[0]?.customColor).toBeUndefined();
  });

  it("reorders named folders and leaves others in place", () => {
    const folders = [
      folder("a", ["s1"]),
      folder("hidden", ["gone"]),
      folder("b", ["s2"]),
      folder("c", ["s3"]),
    ];
    const next = reorderSessionFolders(folders, ["c", "a", "b"]);
    expect(next.map((entry) => entry.id)).toEqual([
      "c",
      "hidden",
      "a",
      "b",
    ]);
    expect(reorderSessionFolders(folders, ["a", "b", "c"])).toBe(folders);
  });
});

describe("applySessionListDrop", () => {
  it("makes a folder when one ungrouped session is dropped on another", () => {
    const { folders, createdId } = applySessionListDrop([], "a", {
      kind: "session",
      id: "b",
    });
    expect(createdId).toBeTruthy();
    expect(folders[0]?.sessionIds).toEqual(["a", "b"]);
  });

  it("joins the target folder when dropping on a session already in one", () => {
    const folders = [folder("work", ["b"])];
    const next = applySessionListDrop(folders, "a", {
      kind: "session",
      id: "b",
    });
    expect(next.createdId).toBeUndefined();
    expect(next.folders[0]?.sessionIds).toEqual(["b", "a"]);
  });

  it("expands a collapsed folder you drop onto", () => {
    const folders = [folder("work", ["b"], { collapsed: true })];
    const next = applySessionListDrop(folders, "a", {
      kind: "folder",
      id: "work",
    });
    expect(next.folders[0]?.collapsed).toBe(false);
    expect(next.folders[0]?.sessionIds).toEqual(["b", "a"]);
  });

  it("ignores a drop onto itself or a sibling in the same folder", () => {
    const folders = [folder("work", ["a", "b"])];
    expect(
      applySessionListDrop(folders, "a", { kind: "session", id: "a" }).folders,
    ).toBe(folders);
    expect(
      applySessionListDrop(folders, "a", { kind: "session", id: "b" }).folders,
    ).toBe(folders);
  });

  it("moves a session from one folder into another", () => {
    const folders = [folder("src", ["a"]), folder("dst", ["b"])];
    const next = applySessionListDrop(folders, "a", {
      kind: "folder",
      id: "dst",
    });
    expect(next.folders.map((entry) => entry.id)).toEqual(["dst"]);
    expect(next.folders[0]?.sessionIds).toEqual(["b", "a"]);
  });
});

describe("pruneSessionFolders", () => {
  it("drops unknown members and empty folders, keeping the same array when nothing changed", () => {
    const folders = [
      folder("keep", ["a", "gone"]),
      folder("empty", ["missing"]),
    ];
    const next = pruneSessionFolders(folders, new Set(["a"]));
    expect(next).toEqual([folder("keep", ["a"])]);
    expect(pruneSessionFolders(next, new Set(["a"]))).toBe(next);
  });
});

describe("folderContaining", () => {
  it("finds the folder a session belongs to", () => {
    const folders = [folder("g", ["a"])];
    expect(folderContaining(folders, "a")?.id).toBe("g");
    expect(folderContaining(folders, "b")).toBeUndefined();
  });
});

describe("session folder persistence", () => {
  beforeEach(() => {
    mockLocalStorage();
  });

  afterEach(() => {
    mockLocalStorage();
  });

  it("round-trips folders for a project and ignores another cwd", () => {
    const folders = [folder("g", ["a"], { name: "Work", collapsed: true })];
    saveSessionFolders("/tmp/project/", folders);
    expect(loadSessionFolders("/tmp/project")).toEqual(folders);
    expect(loadSessionFolders("/tmp/other")).toEqual([]);
  });

  it("does not persist the home cwd", () => {
    saveSessionFolders("~", [folder("g", ["a"])]);
    expect(loadSessionFolders("~")).toEqual([]);
  });

  it("round-trips a folder color", () => {
    const folders = [folder("g", ["a"], { name: "Work", colorIndex: 4 })];
    saveSessionFolders("/tmp/project", folders);
    expect(loadSessionFolders("/tmp/project")[0]?.colorIndex).toBe(4);
  });

  it("round-trips a custom folder color and prefers it over a palette index", () => {
    const folders = [
      folder("g", ["a"], { name: "Work", customColor: "#AABBCC" }),
    ];
    saveSessionFolders("/tmp/project", folders);
    expect(loadSessionFolders("/tmp/project")[0]).toMatchObject({
      customColor: "#aabbcc",
    });
    expect(loadSessionFolders("/tmp/project")[0]?.colorIndex).toBeUndefined();

    saveSessionFolders("/tmp/project", [
      folder("g", ["a"], {
        name: "Work",
        colorIndex: 4,
        customColor: "#ff00aa",
      }),
    ]);
    expect(loadSessionFolders("/tmp/project")[0]).toMatchObject({
      customColor: "#ff00aa",
    });
    expect(loadSessionFolders("/tmp/project")[0]?.colorIndex).toBeUndefined();
  });

  it("drops an invalid custom folder color on load", () => {
    saveSessionFolders("/tmp/project", [
      folder("g", ["a"], { name: "Work", customColor: "red" }),
    ]);
    expect(loadSessionFolders("/tmp/project")[0]?.customColor).toBeUndefined();
  });

  it("drops a project key when the last folder is gone", () => {
    saveSessionFolders("/tmp/project", [folder("g", ["a"])]);
    saveSessionFolders("/tmp/project", []);
    expect(localStorage.getItem("monocode.sessionFolders")).toBe("{}");
  });
});
