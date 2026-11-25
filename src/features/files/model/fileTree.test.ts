import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FsEntry } from "../../../platform/tauri/fs";
import {
  forgetDir,
  listCachedDir,
  notifyDirsChanged,
  peekDir,
  refreshCachedDirs,
  refreshDir,
  subscribeDirsChanged,
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
  const actual = await importOriginal<typeof import("../../../platform/tauri/fs")>();
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

  it("refreshes descendants when the scope is the filesystem root", async () => {
    vi.useFakeTimers();
    listDir.mockResolvedValue([]);
    await listCachedDir("/");
    await listCachedDir(`${root}/nested`);
    listDir.mockClear();

    notifyDirsChanged("/");
    await vi.runAllTimersAsync();

    expect(listDir).toHaveBeenCalledWith("/");
    expect(listDir).toHaveBeenCalledWith(`${root}/nested`);
    forgetDir("/");
  });

  it("matches descendants when the scope uses Windows separators", async () => {
    vi.useFakeTimers();
    const project = "C:\\project";
    const nested = `${project}\\src`;
    listDir.mockResolvedValue([]);
    await listCachedDir(project);
    await listCachedDir(nested);
    listDir.mockClear();

    notifyDirsChanged(project);
    await vi.runAllTimersAsync();

    expect(listDir).toHaveBeenCalledWith(project);
    expect(listDir).toHaveBeenCalledWith(nested);
    forgetDir(project);
    forgetDir(nested);
  });

  it("falls back to all cached folders when one queued scope is unknown", async () => {
    vi.useFakeTimers();
    const parent = "/workspace";
    const known = `${parent}/a`;
    listDir.mockResolvedValue([]);
    await listCachedDir(parent);
    await listCachedDir(known);
    listDir.mockClear();

    notifyDirsChanged(known);
    notifyDirsChanged(`${parent}/b`);
    await vi.runAllTimersAsync();

    expect(listDir).toHaveBeenCalledWith(parent);
    expect(listDir).toHaveBeenCalledWith(known);
    forgetDir(parent);
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
