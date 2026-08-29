import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  discoverPiSkills: vi.fn(),
  listSkills: vi.fn(),
}));

vi.mock("./harness/piSkills", () => ({
  discoverPiSkills: mocks.discoverPiSkills,
}));

vi.mock("./fs", () => ({
  createPath: vi.fn(),
  homeDir: vi.fn(),
  listSkills: mocks.listSkills,
  readTextFile: vi.fn(),
  writeTextFile: vi.fn(),
}));

import {
  BUILTIN_CREATE_SKILL,
  invalidateSkills,
  loadSkills,
  peekSkills,
  skillCatalogKey,
} from "./skills";
import type { PiSkillCommand } from "./harness/piSkills";

function piSkill(name: string): PiSkillCommand {
  return {
    name,
    description: `${name} description`,
    invocation: `skill:${name}`,
    source: "pi",
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-08-29T12:00:00Z"));
  invalidateSkills();
  mocks.discoverPiSkills.mockReset();
  mocks.listSkills.mockReset();
  mocks.discoverPiSkills.mockResolvedValue([piSkill("architect")]);
  mocks.listSkills.mockResolvedValue([]);
});

describe("provider-aware skill catalog", () => {
  it("uses Pi discovery without adding MonoCode's built-in row", async () => {
    const catalog = await loadSkills({ harness: "pi", cwd: "/repo/" });

    expect(mocks.discoverPiSkills).toHaveBeenCalledWith("/repo");
    expect(catalog).toEqual([
      {
        kind: "native",
        ...piSkill("architect"),
      },
    ]);
    expect(catalog).not.toContainEqual(BUILTIN_CREATE_SKILL);
  });

  it("keeps filesystem discovery and the built-in row for non-Pi providers", async () => {
    const catalog = await loadSkills({ harness: "claude", cwd: "/repo" });

    expect(mocks.listSkills).toHaveBeenCalledWith("/repo");
    expect(catalog).toContainEqual(BUILTIN_CREATE_SKILL);
    expect(mocks.discoverPiSkills).not.toHaveBeenCalled();
  });

  it("separates providers and coalesces equivalent Pi directories", async () => {
    const pending = deferred<PiSkillCommand[]>();
    mocks.discoverPiSkills.mockReturnValue(pending.promise);

    const first = loadSkills({ harness: "pi", cwd: "/repo/" });
    const second = loadSkills({ harness: "pi", cwd: "/repo" });
    const claude = loadSkills({ harness: "claude", cwd: "/repo" });

    expect(mocks.discoverPiSkills).toHaveBeenCalledTimes(1);
    expect(skillCatalogKey({ harness: "pi", cwd: "/repo/" })).toBe(
      skillCatalogKey({ harness: "pi", cwd: "/repo" }),
    );
    expect(skillCatalogKey({ harness: "pi", cwd: "/repo" })).not.toBe(
      skillCatalogKey({ harness: "claude", cwd: "/repo" }),
    );

    pending.resolve([piSkill("architect")]);
    await expect(first).resolves.toEqual(await second);
    await claude;
  });

  it("refreshes stale Pi data and retains it after a failed refresh", async () => {
    await loadSkills({ harness: "pi", cwd: "/repo" });
    vi.advanceTimersByTime(30_001);
    const refresh = deferred<PiSkillCommand[]>();
    mocks.discoverPiSkills.mockReturnValueOnce(refresh.promise);

    const loading = loadSkills({ harness: "pi", cwd: "/repo" });
    expect(peekSkills({ harness: "pi", cwd: "/repo" })?.[0]?.name).toBe(
      "architect",
    );
    refresh.resolve([piSkill("new-skill")]);
    await expect(loading).resolves.toMatchObject([{ name: "new-skill" }]);

    vi.advanceTimersByTime(30_001);
    mocks.discoverPiSkills.mockRejectedValueOnce(new Error("offline"));
    await expect(
      loadSkills({ harness: "pi", cwd: "/repo" }),
    ).resolves.toMatchObject([{ name: "new-skill" }]);
    await loadSkills({ harness: "pi", cwd: "/repo" });
    expect(mocks.discoverPiSkills).toHaveBeenCalledTimes(3);

    vi.advanceTimersByTime(5_001);
    await loadSkills({ harness: "pi", cwd: "/repo" });
    expect(mocks.discoverPiSkills).toHaveBeenCalledTimes(4);
  });

  it("does not let an invalidated request replace a newer generation", async () => {
    const old = deferred<PiSkillCommand[]>();
    const current = deferred<PiSkillCommand[]>();
    mocks.discoverPiSkills
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(current.promise);

    const oldLoad = loadSkills({ harness: "pi", cwd: "/repo" });
    invalidateSkills({ cwd: "/repo" });
    const currentLoad = loadSkills(
      { harness: "pi", cwd: "/repo" },
      { refresh: true },
    );
    current.resolve([piSkill("current")]);
    await currentLoad;
    old.resolve([piSkill("old")]);

    await expect(oldLoad).resolves.toMatchObject([{ name: "current" }]);
    expect(peekSkills({ harness: "pi", cwd: "/repo" })).toMatchObject([
      { name: "current" },
    ]);
  });

  it("rejects completions captured before a global reset", async () => {
    const old = deferred<PiSkillCommand[]>();
    mocks.discoverPiSkills.mockReturnValueOnce(old.promise);
    const oldLoad = loadSkills({ harness: "pi", cwd: "/repo" });

    invalidateSkills();
    mocks.discoverPiSkills.mockResolvedValueOnce([piSkill("current")]);
    await loadSkills({ harness: "pi", cwd: "/repo" });
    old.resolve([piSkill("old")]);

    await expect(oldLoad).resolves.toMatchObject([{ name: "current" }]);
    expect(peekSkills({ harness: "pi", cwd: "/repo" })).toMatchObject([
      { name: "current" },
    ]);
  });
});
