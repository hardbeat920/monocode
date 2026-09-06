import { describe, expect, it } from "vitest";
import {
  agentCliCommands,
  agentCliLaunch,
  agentCliPrompt,
  cliCommandInput,
  filterSlashItems,
} from "./agentCli";
import { PLAN_COMMAND } from "./plan";
import { COMPACT_COMMAND } from "./compact";
import type { Skill } from "./skills";

const skill: Skill = {
  kind: "file",
  name: "status",
  invocation: "status",
  description: "Project status",
  path: "/repo/.agents/skills/status/SKILL.md",
  scope: "project",
  source: "agents",
};

describe("CLI command routing", () => {
  it.each(["codex", "claude"] as const)(
    "routes %s commands, arguments and new command names without sending them to the model",
    (harness) => {
      expect(agentCliPrompt("/status", harness, "all", [])).toBe("/status");
      expect(agentCliPrompt("/cli:model opus", harness, "all", [])).toBe(
        "/model opus",
      );
      expect(
        agentCliPrompt("/future-command @file --flag", harness, "commands", []),
      ).toBe("/future-command @file --flag");
      expect(
        agentCliPrompt("Discuss /status", harness, "commands", []),
      ).toBeNull();
      expect(agentCliPrompt("> /status", harness, "commands", [])).toBeNull();
    },
  );
  it("preserves skills and MonoCode commands and allows an explicit native choice", () => {
    expect(agentCliPrompt("/status", "claude", "all", [skill])).toBeNull();
    expect(agentCliPrompt("/cli:status", "claude", "skills", [skill])).toBe(
      "/status",
    );
    expect(agentCliPrompt("/status", "claude", "commands", [skill])).toBe(
      "/status",
    );
    expect(agentCliPrompt("/compact", "codex", "all", [])).toBeNull();
    expect(agentCliPrompt("/compact", "codex", "commands", [])).toBe(
      "/compact",
    );
    expect(agentCliPrompt("/plan fix it", "codex", "all", [])).toBeNull();
    expect(agentCliPrompt("/cli:plan fix it", "codex", "all", [])).toBe(
      "/plan fix it",
    );
    expect(agentCliPrompt("/status", "codex", "skills", [])).toBeNull();
    expect(agentCliPrompt("/status", "omp", "commands", [])).toBeNull();
  });
  it("filters commands and skills without capping the available commands", () => {
    const commands = agentCliCommands("claude");
    const items = [skill, PLAN_COMMAND, COMPACT_COMMAND, ...commands];
    expect(commands.length).toBeGreaterThan(50);
    expect(filterSlashItems(items, "skills")).toEqual([skill]);
    expect(filterSlashItems(items, "commands")).not.toContain(skill);
    expect(filterSlashItems(items, "commands")).toEqual(commands);
    expect(filterSlashItems(items, "all")).toEqual(items);
    expect(new Set(commands.map((item) => item.invocation)).size).toBe(
      commands.length,
    );
    expect(agentCliCommands("cursor")).toEqual([]);
  });
});

describe("native CLI launch", () => {
  it("forks existing history so CLI and chat never write the same conversation", () => {
    const id = "a71674e4-0281-4c5e-9b77-0f4dc5e637ff";
    expect(
      agentCliLaunch(
        { harness: "codex", providerSessionId: id },
        "/path with spaces/codex",
      ),
    ).toEqual({ program: "/path with spaces/codex", args: ["fork", id] });
    expect(
      agentCliLaunch(
        { harness: "claude", providerSessionId: id },
        "/bin/claude",
      ),
    ).toEqual({
      program: "/bin/claude",
      args: ["--resume", id, "--fork-session"],
    });
  });
  it("starts fresh when a provider switch makes the old session incompatible", () => {
    expect(
      agentCliLaunch(
        {
          harness: "claude",
          providerSessionId: "old-codex-id",
          pendingSwitch: {
            from: "codex",
            fromModel: "codex:test",
            fromSettings: {},
          },
        },
        "/bin/claude",
      ).args,
    ).toEqual([]);
    expect(agentCliLaunch({ harness: "codex" }, "/bin/codex").args).toEqual([]);
  });
  it("does not interpret user data as shell code or use permission bypass flags", () => {
    const launch = agentCliLaunch(
      { harness: "codex", providerSessionId: "literal; $(do-not-run)" },
      "/bin/codex",
    );
    expect(launch.args).toEqual(["fork", "literal; $(do-not-run)"]);
    expect(launch.args.join(" ")).not.toContain("bypass");
    expect(() => agentCliLaunch({ harness: "cursor" }, "/bin/agent")).toThrow();
  });
  it("rejects terminal escapes and extra Enter presses while preserving legitimate command arguments", () => {
    expect(cliCommandInput('/model "some model"')).toBe('/model "some model"');
    for (const value of [
      "/status\r/logout",
      "/status\n/model",
      "/status\x1b[A",
      "/status\t",
      "status",
      "/status\0",
    ]) {
      expect(() => cliCommandInput(value)).toThrow();
    }
  });
});
