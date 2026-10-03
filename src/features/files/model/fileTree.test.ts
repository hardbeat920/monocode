import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FsEntry } from "../../../platform/tauri/fs";
import {
  bulkError,
  forgetDir,
  listCachedDir,
  notifyDirsChanged,
  peekDir,
  refreshCachedDirs,
  refreshDir,
  subscribeDirsChanged,
  visibleChildren,
  visibleTreeOrder,
} from "./fileTree";

const root = "/tmp/empty-project";

function entry(name: string): FsEntry {
  return {
    name,
    path: `${root}/${name}`,
    isDir: false,
    ignored: false,
  };
}

const listDir = vi.fn<(path: string) => Promise<FsEntry[]>>();

vi.mock("../../../platform/tauri/fs", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../../platform/tauri/fs")>();
  return {
    ...actual,
    listDir: (path: string) => listDir(path),
  };
});

describe("fileTree cache", () => {
  beforeEach(() => {
    forgetDir(root);
    listDir.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("keeps the first listing until refreshDir", async () => {
    listDir.mockResolvedValueOnce([]);
    await listCachedDir(root);
    expect(peekDir(root)).toEqual([]);

    listDir.mockResolvedValueOnce([entry("hello.ts")]);
    expect(await listCachedDir(root)).toEqual([]);
    expect(listDir).toHaveBeenCalledTimes(1);

    expect(await refreshDir(root)).toEqual([entry("hello.ts")]);
    expect(peekDir(root)).toEqual([entry("hello.ts")]);
  });

  it("refreshCachedDirs re-lists every cached folder", async () => {
    listDir.mockResolvedValueOnce([]);
    await listCachedDir(root);
    listDir.mockResolvedValueOnce([entry("created.ts")]);
    await refreshCachedDirs();
    expect(peekDir(root)).toEqual([entry("created.ts")]);
  });

  it("notifyDirsChanged refreshes the cache and tells listeners", async () => {
    vi.useFakeTimers();
    listDir.mockResolvedValueOnce([]);
    await listCachedDir(root);

    const onChange = vi.fn();
    const stop = subscribeDirsChanged(onChange);
    listDir.mockResolvedValueOnce([entry("from-agent.ts")]);
    notifyDirsChanged();
    expect(peekDir(root)).toEqual([]);

    await vi.runAllTimersAsync();
    expect(peekDir(root)).toEqual([entry("from-agent.ts")]);
    expect(onChange).toHaveBeenCalledTimes(1);
    stop();
  });
});

const proj = "/tmp/tree-project";

function node(path: string, isDir = false, ignored = false): FsEntry {
  return {
    name: path.slice(path.lastIndexOf("/") + 1),
    path: `${proj}/${path}`,
    isDir,
    ignored,
  };
}

/** Caches listings through the module's own loader. */
async function seed(listings: Record<string, FsEntry[]>) {
  listDir.mockImplementation((path) => {
    const hit = listings[path === proj ? "" : path.slice(proj.length + 1)];
    return hit ? Promise.resolve(hit) : Promise.reject(new Error("missing"));
  });
  for (const rel of Object.keys(listings)) {
    await listCachedDir(rel ? `${proj}/${rel}` : proj);
  }
}

const p = (...rels: string[]) => rels.map((rel) => `${proj}/${rel}`);

describe("visibleChildren", () => {
  const entries = [
    node("b.ts"),
    node("src", true),
    node("dist", true, true),
    node("a.log", false, true),
    node("lib", true),
  ];

  it("puts folders before files, keeping listing order", () => {
    expect(visibleChildren(entries, true)).toEqual({
      folders: [node("src", true), node("dist", true, true), node("lib", true)],
      files: [node("b.ts"), node("a.log", false, true)],
    });
  });

  it("hides ignored entries unless shown", () => {
    expect(visibleChildren(entries, false)).toEqual({
      folders: [node("src", true), node("lib", true)],
      files: [node("b.ts")],
    });
  });
});

describe("visibleTreeOrder", () => {
  beforeEach(async () => {
    forgetDir(proj);
    listDir.mockReset();
    await seed({
      "": [
        node("README.md"),
        node("src", true),
        node("dist", true, true),
        node("docs", true),
        node(".env", false, true),
      ],
      src: [node("src/main.ts"), node("src/lib", true)],
      "src/lib": [node("src/lib/util.ts")],
      dist: [node("dist/out.js")],
    });
  });

  it("lists only the root's rows when nothing else is expanded", () => {
    expect(visibleTreeOrder(proj, new Set([proj]), false)).toEqual(
      p("src", "docs", "README.md"),
    );
  });

  it("is empty when the root is collapsed or not loaded", () => {
    expect(visibleTreeOrder(proj, new Set(), false)).toEqual([]);
    forgetDir(proj);
    expect(visibleTreeOrder(proj, new Set([proj]), false)).toEqual([]);
  });

  it("nests expanded folders, folders before files at each level", () => {
    const expanded = new Set([proj, ...p("src", "src/lib")]);
    expect(visibleTreeOrder(proj, expanded, false)).toEqual(
      p(
        "src",
        "src/lib",
        "src/lib/util.ts",
        "src/main.ts",
        "docs",
        "README.md",
      ),
    );
  });

  it("hides descendants of a collapsed folder even if they are expanded", () => {
    const expanded = new Set([proj, ...p("src/lib")]);
    expect(visibleTreeOrder(proj, expanded, false)).toEqual(
      p("src", "docs", "README.md"),
    );
  });

  it("treats an expanded but unloaded folder as empty", () => {
    const expanded = new Set([proj, ...p("docs")]);
    expect(visibleTreeOrder(proj, expanded, false)).toEqual(
      p("src", "docs", "README.md"),
    );
  });

  it("includes ignored rows and their children when shown", () => {
    const expanded = new Set([proj, ...p("dist")]);
    expect(visibleTreeOrder(proj, expanded, true)).toEqual(
      p("src", "dist", "dist/out.js", "docs", "README.md", ".env"),
    );
    expect(visibleTreeOrder(proj, expanded, false)).toEqual(
      p("src", "docs", "README.md"),
    );
  });
});

describe("bulkError", () => {
  it("is null when nothing failed", () => {
    expect(bulkError([])).toBeNull();
  });

  it("keeps a single failure's message", () => {
    expect(bulkError([{ item: "/a", error: new Error("nope") }])?.message).toBe(
      "nope",
    );
  });

  it("counts several failures and shows the first message", () => {
    expect(
      bulkError([
        { item: "/a", error: new Error("nope") },
        { item: "/b", error: "busy" },
      ])?.message,
    ).toBe("2 items failed. nope");
  });
});
