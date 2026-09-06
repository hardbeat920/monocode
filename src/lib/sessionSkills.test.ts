vi.mock("./harness/registry", () => ({
  getHarness: (id: string) =>
    id === "pi" || id === "omp"
      ? {
          commands: {
            discover: async () => [],
            rawSlashCommands: id === "omp",
          },
        }
      : undefined,
}));

import { describe, expect, it, vi } from "vitest";
import { nativeSkillContextForSession } from "./sessionSkills";

describe("nativeSkillContextForSession", () => {
  it("scopes OMP warmup to the active conversation worktree", () => {
    expect(
      nativeSkillContextForSession({
        id: "thread",
        harness: "omp",
        cwd: "/repo",
        worktreeCwd: "/worktree",
      }),
    ).toEqual({ harness: "omp", cwd: "/worktree", sessionId: "thread" });
  });
  it("uses a Pi session worktree", () => {
    expect(
      nativeSkillContextForSession({
        harness: "pi",
        cwd: "/repo",
        worktreeCwd: "/repo-worktree",
      }),
    ).toEqual({ harness: "pi", cwd: "/repo-worktree" });
  });

  it("ignores a non-Pi session", () => {
    expect(
      nativeSkillContextForSession({ harness: "claude", cwd: "/repo" }),
    ).toBeNull();
  });
});
