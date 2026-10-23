import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  resolveWorktreesFolder,
  saveProjectWorktreesFolder,
  saveWorktreesFolder,
} from "./worktreeLocation";

describe("worktree folder", () => {
  beforeEach(() => {
    const storage = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value);
      },
      removeItem: (key: string) => {
        storage.delete(key);
      },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("prefers the project folder, then the global folder per project, then the default", () => {
    const fallback = "/code/app-worktrees";
    const unset = resolveWorktreesFolder("/code/app", fallback);
    expect(unset).toBe(fallback);

    saveWorktreesFolder("/code/worktrees");
    const global = resolveWorktreesFolder("/code/app", fallback);
    expect(global).toBe("/code/worktrees/app");

    saveProjectWorktreesFolder("/code/app/", "/fast/app-trees");
    const project = resolveWorktreesFolder("/code/app", fallback);
    const otherProject = resolveWorktreesFolder("/code/other", fallback);
    expect(project).toBe("/fast/app-trees");
    expect(otherProject).toBe("/code/worktrees/other");

    saveProjectWorktreesFolder("/code/app", null);
    saveWorktreesFolder(null);
    const reset = resolveWorktreesFolder("/code/app", fallback);
    expect(reset).toBe(fallback);
  });
});
