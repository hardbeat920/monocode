import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  discoverPiSkills: vi.fn(),
  discoverOmpCommands: vi.fn(),
  subscribe: vi.fn(),
  listSkills: vi.fn(),
  readTextFile: vi.fn(),
}));

vi.mock("./harness/registry", () => ({
  getHarness: (id: string) =>
    id === "pi"
      ? {
          commands: {
            discover: ({ cwd }: { cwd: string }) => mocks.discoverPiSkills(cwd),
          },
        }
      : id === "omp"
        ? {
            commands: {
              discover: mocks.discoverOmpCommands,
              subscribe: mocks.subscribe,
              rawSlashCommands: true,
            },
          }
        : undefined,
}));

vi.mock("./fs", () => ({
  createPath: vi.fn(),
  homeDir: vi.fn(),
  listSkills: mocks.listSkills,
  readTextFile: mocks.readTextFile,
  writeTextFile: vi.fn(),
}));

import {
  loadDisabledSkillPaths,
  saveDisabledSkillPaths,
  SKILLS_CHANGE_EVENT,
  BUILTIN_CREATE_SKILL,
  invalidateSkills,
  loadSkills,
  peekSkills,
  skillCatalogKey,
  subscribeSkills,
  applySkillsToTurn,
} from "./skills";
import type { DiscoveredSkill } from "./fs";
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
  mocks.discoverOmpCommands.mockReset();
  mocks.discoverOmpCommands.mockResolvedValue([
    {
      name: "workflow",
      invocation: "workflow",
      description: "",
      source: "omp",
    },
  ]);
  mocks.subscribe.mockReset();
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

  it("uses OMP native discovery and leaves commands and arguments out of skill injection", async () => {
    const context = {
      harness: "omp" as const,
      cwd: "/repo",
      sessionId: "thread",
    };
    await expect(loadSkills(context)).resolves.toMatchObject([
      { kind: "native", source: "omp", name: "workflow" },
    ]);
    expect(mocks.discoverOmpCommands).toHaveBeenCalledWith(context);
    expect(mocks.listSkills).not.toHaveBeenCalled();
    await expect(
      applySkillsToTurn("/workflow foo /create-skill", context),
    ).resolves.toBe("/workflow foo /create-skill");
  });

  it("isolates OMP sessions while retaining Pi's shared project cache", () => {
    for (const harness of ["pi", "omp"] as const) {
      const a = skillCatalogKey({ harness, cwd: "/repo", sessionId: "a" });
      const b = skillCatalogKey({ harness, cwd: "/repo", sessionId: "b" });
      expect(a === b).toBe(harness === "pi");
    }
  });

  it("a live command update supersedes an older probe and refreshes subscribers", async () => {
    const context = {
      harness: "omp" as const,
      cwd: "/repo",
      sessionId: "thread",
    };
    const probe = deferred<unknown>();
    mocks.discoverOmpCommands.mockReturnValue(probe.promise);
    const pending = loadSkills(context);
    const onSkills = vi.fn();
    const unsubscribe = vi.fn();
    mocks.subscribe.mockReturnValue(unsubscribe);
    const stop = subscribeSkills(context, onSkills);
    mocks.subscribe.mock.calls[0]![1]([
      {
        name: "new-workflow",
        invocation: "new-workflow",
        source: "omp",
        description: "",
      },
    ]);
    probe.resolve([
      {
        name: "old-workflow",
        invocation: "old-workflow",
        source: "omp",
        description: "",
      },
    ]);
    await expect(pending).resolves.toMatchObject([{ name: "new-workflow" }]);
    expect(onSkills).toHaveBeenCalledWith(
      expect.arrayContaining([
        expect.objectContaining({ kind: "native", name: "new-workflow" }),
      ]),
    );
    stop();
    expect(unsubscribe).toHaveBeenCalledOnce();
  });

  it("keeps the last OMP inventory on discovery failure without falling back to injected files", async () => {
    const context = { harness: "omp" as const, cwd: "/repo" };
    await loadSkills(context);
    vi.advanceTimersByTime(30_001);
    mocks.discoverOmpCommands.mockRejectedValue(
      new Error("Unsupported command"),
    );
    await expect(loadSkills(context)).resolves.toMatchObject([
      { name: "workflow" },
    ]);
    invalidateSkills();
    await expect(loadSkills(context)).resolves.toEqual([]);
    expect(mocks.listSkills).not.toHaveBeenCalled();
  });

  it("keeps filesystem discovery and the built-in row for non-Pi providers", async () => {
    const catalog = await loadSkills({ harness: "claude", cwd: "/repo" });

    expect(mocks.listSkills).toHaveBeenCalledWith("/repo", []);
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

describe("file skill visibility preferences", () => {
  const path = "/repo/.agents/skills/review/SKILL.md";
  let storage: Map<string, string>;

  beforeEach((): void => {
    storage = new Map();
    vi.stubGlobal("localStorage", {
      getItem: (key: string): string | null => storage.get(key) ?? null,
      setItem: (key: string, value: string): void => {
        storage.set(key, value);
      },
    });
    vi.stubGlobal("window", new EventTarget());
    mocks.listSkills.mockResolvedValue([
      {
        name: "review",
        description: "Review changes",
        path,
        source: "agents",
        scope: "project",
      },
    ]);
  });
  afterEach((): void => {
    vi.unstubAllGlobals();
  });

  it("removes a hidden file from a cached catalog and restores it", async (): Promise<void> => {
    const context = { harness: "claude", cwd: "/repo" } satisfies Parameters<
      typeof loadSkills
    >[0];
    expect(
      (await loadSkills(context)).some((skill) => skill.name === "review"),
    ).toBe(true);
    saveDisabledSkillPaths([path]);
    expect(await loadSkills(context)).toEqual([BUILTIN_CREATE_SKILL]);
    saveDisabledSkillPaths([]);
    expect(
      (await loadSkills(context)).some((skill) => skill.name === "review"),
    ).toBe(true);
  });

  it("does not inject hidden skill content into a submitted turn", async (): Promise<void> => {
    saveDisabledSkillPaths([path]);
    const result = await applySkillsToTurn("/review inspect this", {
      harness: "claude",
      cwd: "/repo",
    });
    expect(result).toBe("/review inspect this");
  });

  it("leaves provider-owned native catalogs intact", async (): Promise<void> => {
    saveDisabledSkillPaths([path]);
    expect(await loadSkills({ harness: "pi", cwd: "/repo" })).toMatchObject([
      { name: "architect", kind: "native" },
    ]);
  });

  it("notifies open views only after persistence succeeds", (): void => {
    const listener = vi.fn();
    window.addEventListener(SKILLS_CHANGE_EVENT, listener);
    saveDisabledSkillPaths([path]);
    expect(loadDisabledSkillPaths()).toEqual([path]);
    expect(listener).toHaveBeenCalledTimes(1);
    vi.stubGlobal("localStorage", {
      setItem: (): never => {
        throw new Error("quota");
      },
    });
    expect(() => saveDisabledSkillPaths([])).toThrow(
      "Could not save skill preferences",
    );
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("tolerates malformed and mixed stored preferences", (): void => {
    storage.set("monocode.disabledSkillPaths", "invalid json");
    expect(loadDisabledSkillPaths()).toEqual([]);
    storage.set(
      "monocode.disabledSkillPaths",
      JSON.stringify([path, null, 42]),
    );
    expect(loadDisabledSkillPaths()).toEqual([path]);
  });

  it("does not restore an old catalog when a scan finishes after hiding a skill", async (): Promise<void> => {
    const projectPath = "/repo/.agents/skills/review/SKILL.md";
    const personalPath = "/home/user/.agents/skills/review/SKILL.md";
    const projectSkill: DiscoveredSkill = {
      name: "review",
      description: "Project review",
      path: projectPath,
      source: "agents",
      scope: "project",
    };
    const personalSkill: DiscoveredSkill = {
      name: "review",
      description: "Personal review",
      path: personalPath,
      source: "agents",
      scope: "user",
    };

    const pending = deferred<DiscoveredSkill[]>();
    mocks.listSkills.mockReturnValueOnce(pending.promise);
    const context = { harness: "claude", cwd: "/repo" } satisfies Parameters<
      typeof loadSkills
    >[0];

    // 1. Start discovery while project skill is enabled
    const oldScan = loadSkills(context);

    // 2. Disable the project winner during the scan
    saveDisabledSkillPaths([projectPath]);

    // 3. Complete a fresh scan that selects the personal file fallback
    mocks.listSkills.mockResolvedValueOnce([personalSkill]);
    await loadSkills(context);
    expect(peekSkills(context)?.find((s) => s.name === "review")).toMatchObject({
      name: "review",
      path: personalPath,
      scope: "user",
    });

    // 4. Resolve older scan with the obsolete project candidate
    pending.resolve([projectSkill]);
    await oldScan;

    // Assert that the personal fallback remains active
    expect(peekSkills(context)?.find((s) => s.name === "review")).toMatchObject({
      name: "review",
      path: personalPath,
      scope: "user",
    });

    // 5. Cover re-enabling during a scan:
    const reEnablePending = deferred<DiscoveredSkill[]>();
    mocks.listSkills.mockReturnValueOnce(reEnablePending.promise);
    const inFlightPersonalScan = loadSkills(context, { refresh: true });

    // Re-enable project winner during the scan
    saveDisabledSkillPaths([]);

    // Complete fresh scan returning restored project winner
    mocks.listSkills.mockResolvedValueOnce([projectSkill]);
    await loadSkills(context);
    expect(peekSkills(context)?.find((s) => s.name === "review")).toMatchObject({
      name: "review",
      path: projectPath,
      scope: "project",
    });

    // Resolve the in-flight older scan
    reEnablePending.resolve([personalSkill]);
    await inFlightPersonalScan;

    // Assert that project winner remains active
    expect(peekSkills(context)?.find((s) => s.name === "review")).toMatchObject({
      name: "review",
      path: projectPath,
      scope: "project",
    });
  });

  it("falls back to same-name personal skill when project skill is disabled, and injects its content", async (): Promise<void> => {
    const projectSkillPath = "/repo/.agents/skills/review/SKILL.md";
    const personalSkillPath = "/home/user/.agents/skills/review/SKILL.md";
    const context = { harness: "claude", cwd: "/repo" } satisfies Parameters<
      typeof loadSkills
    >[0];

    mocks.listSkills.mockImplementation(
      async (_cwd: string, disabled?: readonly string[] | null) => {
        const disabledSet = new Set(disabled ?? []);
        if (!disabledSet.has(projectSkillPath)) {
          return [
            {
              name: "review",
              description: "Project review",
              path: projectSkillPath,
              source: "agents",
              scope: "project",
            },
          ];
        }
        if (!disabledSet.has(personalSkillPath)) {
          return [
            {
              name: "review",
              description: "Personal review",
              path: personalSkillPath,
              source: "agents",
              scope: "user",
            },
          ];
        }
        return [];
      },
    );

    mocks.readTextFile.mockImplementation(async (targetPath: string) => {
      if (targetPath === projectSkillPath) return "Project review instructions";
      if (targetPath === personalSkillPath) return "Personal review instructions";
      return "";
    });

    // 1. With neither disabled, project file wins
    const initialSkills = await loadSkills(context);
    const initialReview = initialSkills.find((s) => s.name === "review");
    expect(initialReview).toMatchObject({
      name: "review",
      path: projectSkillPath,
      scope: "project",
    });
    const initialTurn = await applySkillsToTurn("/review inspect this", context);
    expect(initialTurn).toContain("Project review instructions");

    // 2. Disabling only the project file makes the personal file the active result
    saveDisabledSkillPaths([projectSkillPath]);
    const fallbackSkills = await loadSkills(context);
    const fallbackReview = fallbackSkills.find((s) => s.name === "review");
    expect(fallbackReview).toMatchObject({
      name: "review",
      path: personalSkillPath,
      scope: "user",
    });
    const fallbackTurn = await applySkillsToTurn("/review inspect this", context);
    expect(fallbackTurn).toContain("Personal review instructions");
    expect(fallbackTurn).not.toContain("Project review instructions");

    // 3. Re-enabling the project file restores it as the winner
    saveDisabledSkillPaths([]);
    const restoredSkills = await loadSkills(context);
    const restoredReview = restoredSkills.find((s) => s.name === "review");
    expect(restoredReview).toMatchObject({
      name: "review",
      path: projectSkillPath,
      scope: "project",
    });
    const restoredTurn = await applySkillsToTurn("/review inspect this", context);
    expect(restoredTurn).toContain("Project review instructions");

    // 4. Disabling lower-priority candidate does not affect enabled winner
    saveDisabledSkillPaths([personalSkillPath]);
    const winnerSkills = await loadSkills(context);
    const winnerReview = winnerSkills.find((s) => s.name === "review");
    expect(winnerReview).toMatchObject({
      name: "review",
      path: projectSkillPath,
      scope: "project",
    });

    // 5. Disabling both files removes that file skill from the active catalog
    saveDisabledSkillPaths([projectSkillPath, personalSkillPath]);
    const disabledSkills = await loadSkills(context);
    expect(disabledSkills.find((s) => s.name === "review")).toBeUndefined();
    const disabledTurn = await applySkillsToTurn("/review inspect this", context);
    expect(disabledTurn).toBe("/review inspect this");
  });
});
