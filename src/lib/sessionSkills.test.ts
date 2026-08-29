import { describe, expect, it } from "vitest";
import { piSkillContextForSession } from "./sessionSkills";

describe("piSkillContextForSession", () => {
  it("uses a Pi session worktree", () => {
    expect(
      piSkillContextForSession({
        harness: "pi",
        cwd: "/repo",
        worktreeCwd: "/repo-worktree",
      }),
    ).toEqual({ harness: "pi", cwd: "/repo-worktree" });
  });

  it("ignores a non-Pi session", () => {
    expect(
      piSkillContextForSession({ harness: "claude", cwd: "/repo" }),
    ).toBeNull();
  });
});
