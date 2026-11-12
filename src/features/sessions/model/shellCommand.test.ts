import { describe, expect, it } from "vitest";
import type { Block, Session } from "./session";
import {
  appendShellCommand,
  consumeShellCommand,
  finishShellCommand,
  sanitizeShellCommand,
  shellCommandRunning,
  withShellCommands,
} from "./shellCommand";
import { groupTurns } from "./transcriptActivity";

const session = (blocks: Block[] = []): Session =>
  ({ id: "s", harness: "claude", cwd: "/repo", blocks }) as unknown as Session;

const ran = (
  command: string,
  output: Partial<{ stdout: string; stderr: string; exitCode: number }> = {},
) =>
  finishShellCommand(
    appendShellCommand(session(), command, command, "/repo/wt", 1_000),
    command,
    {
      stdout: output.stdout ?? "",
      stderr: output.stderr ?? "",
      exitCode: output.exitCode ?? 0,
      truncated: false,
      stopped: false,
    },
    1_250,
  ).blocks[0];

describe("consumeShellCommand", () => {
  it("takes the command after a leading !", () => {
    expect(consumeShellCommand("!git status")).toBe("git status");
    expect(consumeShellCommand("! npm test \n")).toBe("npm test");
    expect(consumeShellCommand("!")).toBeNull();
    expect(consumeShellCommand("Done!")).toBeNull();
    expect(consumeShellCommand(" !ls")).toBeNull();
  });
});

describe("shell command blocks", () => {
  it("runs, then records the output, exit code and duration", () => {
    const started = appendShellCommand(session(), "r1", "ls", "/repo/wt", 10);
    expect(shellCommandRunning(started)).toBe(true);
    const done = finishShellCommand(
      started,
      "r1",
      {
        stdout: "a\n",
        stderr: "",
        exitCode: 2,
        truncated: true,
        stopped: false,
      },
      40,
    );
    expect(shellCommandRunning(done)).toBe(false);
    expect(done.blocks[0]).toMatchObject({
      role: "shell",
      text: "ls",
      startedAt: 10,
      durationMs: 30,
      shell: {
        cwd: "/repo/wt",
        status: "done",
        stdout: "a\n",
        exitCode: 2,
        truncated: true,
      },
    });
  });

  it("marks a stopped command and one that never started", () => {
    const started = appendShellCommand(session(), "r1", "sleep 9", "/repo");
    expect(
      finishShellCommand(started, "r1", {
        stdout: "",
        stderr: "",
        exitCode: null,
        truncated: false,
        stopped: true,
      }).blocks[0].shell,
    ).toEqual({ cwd: "/repo", status: "stopped", stdout: "", stderr: "" });
    expect(
      finishShellCommand(started, "r1", { error: "/repo is not a folder" })
        .blocks[0].shell,
    ).toEqual({
      cwd: "/repo",
      status: "failed",
      stderr: "/repo is not a folder",
    });
  });

  it("sits on its own row between turns", () => {
    const user: Block = { id: "u", role: "user", text: "hi" };
    const reply: Block = { id: "a", role: "assistant", text: "hello" };
    const command = ran("git status");
    expect(
      groupTurns([user, reply, command]).map((turn) => turn.length),
    ).toEqual([2, 1]);
  });
});

describe("withShellCommands", () => {
  it("hands the agent the commands run since the last message", () => {
    const old = ran("pwd", { stdout: "/repo\n" });
    const user: Block = { id: "u", role: "user", text: "hi" };
    const draft: Block = { id: "d", role: "user", text: "later", draft: true };
    const status = ran("git status", { stdout: "clean\n" });
    const test = ran("npm test", { stderr: "1 failed\n", exitCode: 1 });
    expect(
      withShellCommands([old, user, status, draft, test], "Why did it fail?"),
    ).toBe(
      [
        "I ran these shell commands myself before this message:",
        "",
        "<bash-input>git status</bash-input>",
        "<bash-stdout>clean\n</bash-stdout>",
        "<bash-stderr></bash-stderr>",
        "Exit code 0.",
        "",
        "<bash-input>npm test</bash-input>",
        "<bash-stdout></bash-stdout>",
        "<bash-stderr>1 failed\n</bash-stderr>",
        "Exit code 1.",
        "",
        "Why did it fail?",
      ].join("\n"),
    );
  });

  it("leaves the prompt alone when nothing ran or a command is still running", () => {
    const user: Block = { id: "u", role: "user", text: "hi" };
    const running = appendShellCommand(session(), "r", "make", "/repo")
      .blocks[0];
    expect(withShellCommands([ran("ls"), user], "next")).toBe("next");
    expect(withShellCommands([user, running], "next")).toBe("next");
  });
});

describe("sanitizeShellCommand", () => {
  it("keeps the record and reads a command a quit cut short as stopped", () => {
    const shell = { cwd: "/repo", status: "running" as const };
    expect(sanitizeShellCommand(shell, false)).toEqual(shell);
    expect(sanitizeShellCommand(shell, true)).toEqual({
      cwd: "/repo",
      status: "stopped",
    });
    expect(
      sanitizeShellCommand(ran("ls", { stdout: "a" }).shell, true),
    ).toEqual({
      cwd: "/repo/wt",
      status: "done",
      stdout: "a",
      stderr: "",
      exitCode: 0,
    });
    expect(sanitizeShellCommand({ status: "done" }, true)).toBeNull();
  });
});
