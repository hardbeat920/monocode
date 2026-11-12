import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  killChild: vi.fn(),
  onLine: undefined as ((line: string) => void) | undefined,
  onExit: undefined as (() => void) | undefined,
  resolveClaudeBinary: vi.fn(),
  spawnChild: vi.fn(),
  unwatchChild: vi.fn(),
  watchChild: vi.fn(),
  writeChild: vi.fn(),
}));

vi.mock("../../core/child", () => ({
  killChild: mocks.killChild,
  resolveClaudeBinary: mocks.resolveClaudeBinary,
  spawnChild: mocks.spawnChild,
  unwatchChild: mocks.unwatchChild,
  watchChild: mocks.watchChild,
  writeChild: mocks.writeChild,
}));

import {
  claudeCommandProvider,
  claudePromptText,
  claudeCommandsFromInitialize,
  discoverClaudeCommands,
  leadingClaudeCommand,
} from "./claudeCommands";

function initializeResponse(response: Record<string, unknown>): string {
  return JSON.stringify({
    type: "control_response",
    response: {
      subtype: "success",
      request_id: "monocode_commands_init",
      response,
    },
  });
}

describe("claudeCommandsFromInitialize", () => {
  it("maps names, aliases, and argument hints", () => {
    expect(
      claudeCommandsFromInitialize({
        commands: [
          {
            name: "code-review",
            description: "Review the current diff",
            argumentHint: "[low|medium|high] [--fix]",
            aliases: ["review"],
            builtin: true,
          },
          { name: "commit", description: "Commit", argumentHint: "" },
        ],
      }),
    ).toEqual([
      {
        name: "code-review",
        invocation: "code-review",
        source: "claude",
        description: "Review the current diff",
        origin: "builtin",
        aliases: ["review"],
        inputHint: "[low|medium|high] [--fix]",
      },
      {
        name: "commit",
        invocation: "commit",
        source: "claude",
        description: "Commit",
      },
    ]);
  });

  it("hides commands that change state MonoCode tracks, internals, and duplicates", () => {
    const commands = claudeCommandsFromInitialize({
      commands: [
        { name: "clear", aliases: ["reset", "new"] },
        { name: "model" },
        { name: "usage", aliases: ["cost", "stats"] },
        { name: "__remote-workflow" },
        { name: "bad name" },
        { name: "init" },
        { name: "init" },
        { name: "doctor", aliases: ["checkup", "a b", "model"] },
      ],
    });
    expect(commands.map((command) => command.name)).toEqual(["init", "doctor"]);
    expect(commands[1]?.aliases).toEqual(["checkup"]);
  });

  it("namespaces commands that share a name with MonoCode's own, like omp", () => {
    const commands = claudeCommandsFromInitialize({
      commands: [{ name: "compact" }, { name: "mcp" }, { name: "init" }],
    });
    expect(commands.map((command) => command.invocation)).toEqual([
      "claude:compact",
      "claude:mcp",
      "init",
    ]);
  });

  it("sends a namespaced pick to the CLI under its own name", () => {
    expect(claudePromptText("/claude:compact keep the plan")).toBe(
      "/compact keep the plan",
    );
    expect(claudePromptText("/claude:mcp reconnect")).toBe("/mcp reconnect");
    expect(claudePromptText("/claude:init")).toBe("/claude:init");
    expect(claudePromptText("see /claude:compact")).toBe("see /claude:compact");
  });

  it("rejects a payload without commands", () => {
    expect(() => claudeCommandsFromInitialize({})).toThrow(/no commands/);
  });
});

describe("discoverClaudeCommands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.resolveClaudeBinary.mockResolvedValue({ path: "/bin/claude" });
    mocks.killChild.mockResolvedValue(undefined);
    mocks.spawnChild.mockResolvedValue(undefined);
    mocks.watchChild.mockImplementation(
      (_id: string, onLine: (line: string) => void, onExit: () => void) => {
        mocks.onLine = onLine;
        mocks.onExit = onExit;
      },
    );
  });

  it("initializes a hookless CLI in the project and reads its commands", async () => {
    mocks.writeChild.mockImplementation(async () => {
      mocks.onLine?.(
        initializeResponse({ commands: [{ name: "simplify" }] }),
      );
    });

    await expect(discoverClaudeCommands("/repo")).resolves.toEqual([
      {
        name: "simplify",
        invocation: "simplify",
        source: "claude",
        description: "",
      },
    ]);
    const [, , args, cwd, account] = mocks.spawnChild.mock.calls[0] ?? [];
    expect(cwd).toBe("/repo");
    expect(account).toEqual({ provider: "claude", id: "default" });
    expect(args).toContain("--no-session-persistence");
    expect(args).toContain("--strict-mcp-config");
    expect(args.join(" ")).toContain('"disableAllHooks":true');
    expect(JSON.parse(mocks.writeChild.mock.calls[0]?.[1])).toMatchObject({
      request: { subtype: "initialize" },
    });
    expect(mocks.killChild).toHaveBeenCalled();
    expect(mocks.unwatchChild).toHaveBeenCalled();
  });

  it("runs the probe under the session's account", async () => {
    mocks.writeChild.mockImplementation(async () => {
      mocks.onLine?.(initializeResponse({ commands: [] }));
    });

    await discoverClaudeCommands("/repo", "work");
    expect(mocks.spawnChild.mock.calls[0]?.[4]).toEqual({
      provider: "claude",
      id: "work",
    });
  });

  it("fails when the CLI exits before answering", async () => {
    mocks.writeChild.mockImplementation(async () => mocks.onExit?.());

    await expect(discoverClaudeCommands("/repo")).rejects.toThrow(/exited/);
    expect(mocks.killChild).toHaveBeenCalled();
  });

  it("leaves no unhandled rejection when the CLI exits before the write fails", async () => {
    const unhandled = vi.fn();
    process.on("unhandledRejection", unhandled);
    mocks.writeChild.mockImplementation(async () => {
      mocks.onExit?.();
      throw new Error("pipe closed");
    });

    await expect(discoverClaudeCommands("/repo")).rejects.toThrow(/pipe closed/);
    await new Promise((resolve) => setTimeout(resolve, 0));
    process.off("unhandledRejection", unhandled);
    expect(unhandled).not.toHaveBeenCalled();
  });

  const report = (names: string[]) =>
    mocks.writeChild.mockImplementationOnce(async () => {
      mocks.onLine?.(
        initializeResponse({ commands: names.map((name) => ({ name })) }),
      );
    });

  it("recognizes reported commands and aliases from the last probe", async () => {
    const repo = { cwd: "/repo" };
    mocks.writeChild.mockImplementationOnce(async () => {
      mocks.onLine?.(
        initializeResponse({
          commands: [
            { name: "code-review", aliases: ["review"] },
            { name: "compact" },
          ],
        }),
      );
    });
    await discoverClaudeCommands("/repo");

    expect(leadingClaudeCommand("/review high", repo)).toBe("review");
    expect(leadingClaudeCommand("  /code-review", repo)).toBe("code-review");
    expect(
      leadingClaudeCommand("/compact", { cwd: "/repo/", accountId: "default" }),
    ).toBe("compact");
    expect(leadingClaudeCommand("/etc is missing hosts", repo)).toBeNull();
    expect(leadingClaudeCommand("please /review", repo)).toBeNull();
  });

  it("treats any leading /name as a command before a project is probed", () => {
    expect(leadingClaudeCommand("/review", { cwd: "/never-probed" })).toBe(
      "review",
    );
  });

  describe("beforeSend", () => {
    const beforeSend = claudeCommandProvider.beforeSend!;

    it("asks the CLI again about a name it hasn't reported", async () => {
      report(["init"]);
      await discoverClaudeCommands("/fresh");
      mocks.spawnChild.mockClear();

      report(["init", "newskill"]);
      await beforeSend("/newskill go", { cwd: "/fresh", effort: "ultrathink" });
      expect(leadingClaudeCommand("/newskill go", { cwd: "/fresh" })).toBe(
        "newskill",
      );
      expect(mocks.spawnChild).toHaveBeenCalledTimes(1);
    });

    it("probes only for an unknown leading name under Ultrathink", async () => {
      report(["init"]);
      await discoverClaudeCommands("/quiet");
      mocks.spawnChild.mockClear();

      await beforeSend("/init", { cwd: "/quiet", effort: "ultrathink" });
      await beforeSend("/other", { cwd: "/quiet", effort: "high" });
      await beforeSend("plain text", { cwd: "/quiet", effort: "ultrathink" });
      expect(mocks.spawnChild).not.toHaveBeenCalled();
    });

    it("trusts a 'not a command' answer for 30 seconds", async () => {
      const now = vi.spyOn(Date, "now").mockReturnValue(1_000_000);
      report(["init"]);
      await discoverClaudeCommands("/tmp-project");
      mocks.spawnChild.mockClear();
      const context = { cwd: "/tmp-project", effort: "ultrathink" };

      report(["init"]);
      await beforeSend("/tmp is full", context);
      await beforeSend("/tmp is still full", context);
      expect(mocks.spawnChild).toHaveBeenCalledTimes(1);
      expect(leadingClaudeCommand("/tmp is full", context)).toBeNull();

      now.mockReturnValue(1_000_000 + 30_000);
      report(["init", "tmp"]);
      await beforeSend("/tmp is full", context);
      expect(mocks.spawnChild).toHaveBeenCalledTimes(2);
      expect(leadingClaudeCommand("/tmp is full", context)).toBe("tmp");
      now.mockRestore();
    });

    it("backs off for 30 seconds after a failed check", async () => {
      const now = vi.spyOn(Date, "now").mockReturnValue(2_000_000);
      const context = { cwd: "/slow", effort: "ultrathink" };
      mocks.writeChild.mockImplementationOnce(async () => mocks.onExit?.());
      await beforeSend("/thing", context);
      await beforeSend("/thing", context);
      expect(mocks.spawnChild).toHaveBeenCalledTimes(1);
      expect(leadingClaudeCommand("/thing", context)).toBe("thing");

      now.mockReturnValue(2_000_000 + 30_000);
      report(["init"]);
      await beforeSend("/thing", context);
      expect(mocks.spawnChild).toHaveBeenCalledTimes(2);
      expect(leadingClaudeCommand("/thing", context)).toBeNull();
      now.mockRestore();
    });

    it("keeps a list another probe saved while a check was failing", async () => {
      const context = { cwd: "/racy", effort: "ultrathink" };
      let failFirst: (() => void) | undefined;
      mocks.writeChild.mockImplementationOnce(async () => {
        failFirst = mocks.onExit;
      });
      const failing = beforeSend("/thing", context);
      await vi.waitFor(() => expect(failFirst).toBeDefined());

      report(["init"]);
      await discoverClaudeCommands("/racy");
      failFirst?.();
      await failing;

      expect(leadingClaudeCommand("/thing", context)).toBeNull();
      expect(leadingClaudeCommand("/init", context)).toBe("init");
    });

    it("forgets a stale list when the CLI can't be asked", async () => {
      report(["init"]);
      await discoverClaudeCommands("/flaky");
      expect(leadingClaudeCommand("/newskill", { cwd: "/flaky" })).toBeNull();

      mocks.writeChild.mockImplementationOnce(async () => mocks.onExit?.());
      await beforeSend("/newskill", { cwd: "/flaky", effort: "ultrathink" });
      expect(leadingClaudeCommand("/newskill", { cwd: "/flaky" })).toBe(
        "newskill",
      );
    });
  });

  it("keeps each project's and account's commands apart", async () => {
    report(["deploy"]);
    await discoverClaudeCommands("/project-a");
    report(["init"]);
    await discoverClaudeCommands("/project-b");
    report(["init"]);
    await discoverClaudeCommands("/project-a", "work");

    expect(leadingClaudeCommand("/deploy", { cwd: "/project-a" })).toBe("deploy");
    expect(leadingClaudeCommand("/deploy", { cwd: "/project-b" })).toBeNull();
    expect(
      leadingClaudeCommand("/deploy", { cwd: "/project-a", accountId: "work" }),
    ).toBeNull();
  });
});
