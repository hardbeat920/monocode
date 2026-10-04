import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FsEntry } from "../../../platform/tauri/fs";
import {
  forgetDir,
  listCachedDir,
  notifyDirsChanged,
  peekDir,
  refreshCachedDirs,
  refreshDir,
  registerExplorer,
  saveExpanded,
  subscribeDirsChanged,
  unregisterExplorer,
  withoutSubtree,
} from "./fileTree";

const root = "/tmp/empty-project";
const otherRoot = "/tmp/other-project";

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
    forgetDir("/tmp");
    unregisterExplorer(root);
    unregisterExplorer(otherRoot);
    saveExpanded(root, new Set());
    saveExpanded(otherRoot, new Set());
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

  it("refreshCachedDirs re-lists the mounted explorer's folders", async () => {
    registerExplorer(root);
    listDir.mockResolvedValueOnce([]);
    await listCachedDir(root);
    listDir.mockResolvedValueOnce([entry("created.ts")]);
    await refreshCachedDirs();
    expect(peekDir(root)).toEqual([entry("created.ts")]);
  });

  it("drops listings nothing on screen can show", async () => {
    const collapsed = `${root}/src`;
    const other = "/tmp/other-project";
    registerExplorer(root);
    saveExpanded(root, new Set([root]));
    listDir.mockResolvedValue([]);
    await listCachedDir(root);
    await listCachedDir(collapsed);
    await listCachedDir(other);
    listDir.mockClear();

    await refreshCachedDirs();

    expect(peekDir(collapsed)).toBeNull();
    expect(peekDir(other)).toBeNull();
    expect(peekDir(root)).not.toBeNull();
    expect(listDir.mock.calls.map(([path]) => path)).toEqual([root]);
  });

  it("keeps expanded folders under a mounted root", async () => {
    const expanded = `${root}/src`;
    registerExplorer(root);
    saveExpanded(root, new Set([root, expanded]));
    listDir.mockResolvedValue([]);
    await listCachedDir(root);
    await listCachedDir(expanded);
    listDir.mockClear();

    await refreshCachedDirs();

    expect(peekDir(expanded)).toEqual([]);
    expect(listDir.mock.calls.map(([path]) => path).sort()).toEqual(
      [expanded, root].sort(),
    );
  });

  it("drops a descendant left behind by a collapsed folder", async () => {
    const collapsed = `${root}/src`;
    const hidden = `${collapsed}/deep`;
    registerExplorer(root);
    saveExpanded(root, new Set([root, hidden]));
    listDir.mockResolvedValue([]);
    await listCachedDir(root);
    await listCachedDir(collapsed);
    await listCachedDir(hidden);
    listDir.mockClear();

    await refreshCachedDirs();
    await refreshCachedDirs();

    expect(peekDir(hidden)).toBeNull();
    expect(peekDir(collapsed)).toBeNull();
    expect(listDir.mock.calls.map(([path]) => path)).toEqual([root, root]);
  });

  it("drops descendants left behind by a collapsed root", async () => {
    const child = `${root}/src`;
    registerExplorer(root);
    // Root collapsed: it drops out of the set, its children stay.
    saveExpanded(root, new Set([child]));
    listDir.mockResolvedValue([]);
    await listCachedDir(root);
    await listCachedDir(child);
    listDir.mockClear();

    await refreshCachedDirs();

    expect(peekDir(root)).not.toBeNull();
    expect(peekDir(child)).toBeNull();
    expect(listDir.mock.calls.map(([path]) => path)).toEqual([root]);
  });

  it("drops a project's folders once its explorer unmounts", async () => {
    registerExplorer(root);
    listDir.mockResolvedValue([]);
    await listCachedDir(root);
    unregisterExplorer(root);
    listDir.mockClear();

    await refreshCachedDirs();

    expect(peekDir(root)).toBeNull();
    expect(listDir).not.toHaveBeenCalled();
  });

  it("keeps a root mounted until its last explorer unmounts", async () => {
    registerExplorer(root);
    registerExplorer(root);
    listDir.mockResolvedValue([]);
    await listCachedDir(root);
    unregisterExplorer(root);
    listDir.mockClear();

    await refreshCachedDirs();

    expect(peekDir(root)).not.toBeNull();
    expect(listDir.mock.calls.map(([path]) => path)).toEqual([root]);

    unregisterExplorer(root);
    listDir.mockClear();
    await refreshCachedDirs();

    expect(peekDir(root)).toBeNull();
    expect(listDir).not.toHaveBeenCalled();
  });

  it("ignores an unregister with no matching register", async () => {
    registerExplorer(root);
    listDir.mockResolvedValue([]);
    await listCachedDir(root);
    unregisterExplorer(root);
    unregisterExplorer(root);
    listDir.mockClear();

    await refreshCachedDirs();

    expect(peekDir(root)).toBeNull();
  });

  it("notifyDirsChanged refreshes the cache and tells listeners", async () => {
    vi.useFakeTimers();
    registerExplorer(root);
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

describe("withoutSubtree", () => {
  const src = `${root}/src`;

  it("drops a deleted folder and its expanded descendants", () => {
    const expanded = new Set([root, src, `${src}/deep`, `${root}/docs`]);

    expect([...withoutSubtree(expanded, src)]).toEqual([root, `${root}/docs`]);
  });

  it("collapsing a folder yields the set its descendants would occupy", () => {
    // What toggle() now stores: a collapse drops the subtree, so re-expanding
    // starts from the folder alone instead of resurrecting stale children.
    const expanded = new Set([root, src, `${src}/deep`]);

    expect([...withoutSubtree(expanded, src)]).toEqual([root]);
  });

  it("keeps a sibling whose name merely starts the same", () => {
    const sibling = `${root}/src-legacy`;
    const expanded = new Set([root, src, sibling]);

    expect([...withoutSubtree(expanded, src)]).toEqual([root, sibling]);
  });

  it("returns the same set when nothing matched", () => {
    const expanded = new Set([root, src]);

    expect(withoutSubtree(expanded, `${root}/docs`)).toBe(expanded);
  });
});
