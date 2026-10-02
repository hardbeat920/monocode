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
