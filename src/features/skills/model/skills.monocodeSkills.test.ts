const mocks = vi.hoisted(() => ({
  beforeSend: vi.fn(),
  discover: vi.fn(),
  listSkills: vi.fn(),
}));

vi.mock("../../../integrations/harness/core/registry", () => ({
  getHarness: (id: string) =>
    id === "claude"
      ? {
          commands: {
            beforeSend: mocks.beforeSend,
            discover: mocks.discover,
            monocodeSkills: true,
          },
        }
      : undefined,
}));

vi.mock("../../../platform/tauri/fs", () => ({
  listSkills: mocks.listSkills,
}));

import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  BUILTIN_CREATE_SKILL,
  applySkillsToTurn,
  invalidateSkills,
  loadSkills,
  prepareNativeCommand,
} from "./skills";
import { CREATE_SKILL_BODY } from "./createSkill";

const fileSkill = (
  name: string,
  path: string,
  source: "claude" | "codex" = "claude",
) => ({
  name,
  description: "",
  path,
  scope: "user" as const,
  source,
});

function stubDisabledPaths(paths: string[]) {
  const store = new Map([["monocode.disabledSkillPaths", JSON.stringify(paths)]]);
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => store.get(key) ?? null,
  });
}

describe("native catalog with MonoCode skill settings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    invalidateSkills();
    stubDisabledPaths([]);
    mocks.discover.mockResolvedValue([
      { name: "commit", description: "", invocation: "commit", source: "claude" },
      { name: "deploy", description: "", invocation: "deploy", source: "claude" },
      {
        name: "simplify",
        description: "",
        invocation: "simplify",
        source: "claude",
        origin: "builtin",
      },
      {
        name: "create-skill",
        description: "theirs",
        invocation: "create-skill",
        source: "claude",
      },
    ]);
  });

  it("hides fully disabled skills and offers MonoCode's create-skill", async () => {
    stubDisabledPaths(["/home/.claude/skills/commit/SKILL.md", "/s/SKILL.md"]);
    mocks.listSkills.mockImplementation(
      async (_cwd: string, disabled: string[]) =>
        [
          fileSkill("commit", "/home/.claude/skills/commit/SKILL.md"),
          fileSkill("deploy", "/home/.claude/skills/deploy/SKILL.md"),
          fileSkill("simplify", "/s/SKILL.md"),
        ].filter((skill) => !disabled.includes(skill.path)),
    );

    const skills = await loadSkills({ harness: "claude", cwd: "/repo" });

    expect(skills.map((skill) => skill.name)).toEqual([
      "create-skill",
      "deploy",
      "simplify",
    ]);
    expect(skills[0]).toBe(BUILTIN_CREATE_SKILL);
  });

  it("keeps a name while a same-name file is still enabled", async () => {
    stubDisabledPaths(["/project/.claude/skills/commit/SKILL.md"]);
    mocks.listSkills.mockImplementation(
      async (_cwd: string, disabled: string[]) =>
        disabled.length
          ? [fileSkill("commit", "/home/.claude/skills/commit/SKILL.md")]
          : [fileSkill("commit", "/project/.claude/skills/commit/SKILL.md")],
    );

    const skills = await loadSkills({ harness: "claude", cwd: "/repo" });

    expect(skills.map((skill) => skill.name)).toContain("commit");
  });

  it("ignores disabled skills from folders Claude doesn't read", async () => {
    stubDisabledPaths(["/home/.codex/skills/deploy/SKILL.md"]);
    mocks.listSkills.mockImplementation(
      async (_cwd: string, disabled: string[]) =>
        [fileSkill("deploy", "/home/.codex/skills/deploy/SKILL.md", "codex")].filter(
          (skill) => !disabled.includes(skill.path),
        ),
    );

    const skills = await loadSkills({ harness: "claude", cwd: "/repo" });

    expect(skills.map((skill) => skill.name)).toContain("deploy");
  });

  it("skips the file scan when nothing is disabled", async () => {
    await loadSkills({ harness: "claude", cwd: "/repo" });
    expect(mocks.listSkills).not.toHaveBeenCalled();
  });

  it("caches each provider account separately", async () => {
    await loadSkills({ harness: "claude", cwd: "/repo", accountId: "work" });
    await loadSkills({ harness: "claude", cwd: "/repo", accountId: "home" });
    expect(mocks.discover).toHaveBeenCalledTimes(2);
    expect(mocks.discover.mock.calls.map(([context]) => context.accountId)).toEqual([
      "work",
      "home",
    ]);
  });

  it("injects the create-skill body but leaves native commands to the CLI", async () => {
    const created = await applySkillsToTurn("/create-skill for deploys", {
      harness: "claude",
      cwd: "/repo",
    });
    expect(created).toContain(CREATE_SKILL_BODY.trim());
    expect(created.endsWith("/create-skill for deploys")).toBe(true);

    await expect(
      applySkillsToTurn("/commit now", { harness: "claude", cwd: "/repo" }),
    ).resolves.toBe("/commit now");
    await expect(
      applySkillsToTurn("/commit and then /create-skill for it", {
        harness: "claude",
        cwd: "/repo",
      }),
    ).resolves.toBe("/commit and then /create-skill for it");
    await expect(
      applySkillsToTurn("Please /create-skill for deploys", {
        harness: "claude",
        cwd: "/repo",
      }),
    ).resolves.toContain(CREATE_SKILL_BODY.trim());
    expect(mocks.discover).not.toHaveBeenCalled();
  });

  it("skips the pre-send check for MonoCode's own create-skill", async () => {
    mocks.beforeSend.mockResolvedValue(undefined);
    const context = { harness: "claude" as const, cwd: "/repo" };

    await prepareNativeCommand("/create-skill for deploys", context, "ultrathink");
    expect(mocks.beforeSend).not.toHaveBeenCalled();

    await prepareNativeCommand("/create-skills-later", context, "ultrathink");
    expect(mocks.beforeSend).toHaveBeenCalledWith("/create-skills-later", {
      cwd: "/repo",
      effort: "ultrathink",
    });
  });
});
