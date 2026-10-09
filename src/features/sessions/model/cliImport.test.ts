import { describe, expect, it } from "vitest";
import type { CliEntry, CliSession } from "../../../platform/tauri/cliSessions";
import { sessionFromCliEntries } from "./cliImport";

const source = (harness: CliSession["harness"]): CliSession => ({
  harness,
  providerSessionId: "11111111-1111-1111-1111-111111111111",
  cwd: "/Users/me/app",
  title: "Fix the login bug",
  createdAt: 1_000,
  updatedAt: 9_000,
  path: "/Users/me/.claude/projects/-Users-me-app/1111.jsonl",
});

describe("sessionFromCliEntries", () => {
  it("keeps the provider session so the next message resumes it", () => {
    const session = sessionFromCliEntries(
      source("claude"),
      [{ kind: "user", text: "Fix the login bug", at: 1_000 }],
      "/Users/me/app",
    );
    expect(session.harness).toBe("claude");
    expect(session.providerSessionId).toBe(
      "11111111-1111-1111-1111-111111111111",
    );
    expect(session.title).toBe("Fix the login bug");
    expect(session.cwd).toBe("/Users/me/app");
    expect(session.busy).toBe(false);
  });

  it("replays turns, reasoning and tools in order", () => {
    const entries: CliEntry[] = [
      { kind: "user", text: "Fix the login bug", at: 1_000 },
      { kind: "reasoning", text: "Check auth.ts", at: 1_500 },
      {
        kind: "tool",
        id: "toolu_1",
        name: "Bash",
        input: { command: "git status" },
        output: "clean",
        failed: false,
        at: 2_000,
      },
      { kind: "assistant", text: "Fixed.", at: 3_000 },
      { kind: "user", text: "Thanks", at: 4_000 },
      { kind: "assistant", text: "Anytime.", at: 5_000 },
    ];
    const session = sessionFromCliEntries(
      source("claude"),
      entries,
      "/Users/me/app",
    );
    expect(session.blocks.map((block) => block.role)).toEqual([
      "user",
      "reasoning",
      "tool",
      "assistant",
      "user",
      "assistant",
    ]);
    const [first, , tool, , second] = session.blocks;
    expect(first.text).toBe("Fix the login bug");
    expect(first.startedAt).toBe(1_000);
    expect(first.durationMs).toBe(3_000);
    expect(second.startedAt).toBe(4_000);
    expect(tool.tool?.callId).toBe("toolu_1");
    expect(tool.tool?.status).toBe("completed");
    expect(tool.tool?.detail).toBe("clean");
    expect(session.blocks.every((block) => !block.streaming)).toBe(true);
  });

  it("titles shell commands from other providers the way the live adapter does", () => {
    const session = sessionFromCliEntries(
      source("codex"),
      [
        { kind: "user", text: "Run the tests", at: 1_000 },
        {
          kind: "tool",
          id: "c1",
          name: "exec_command",
          command: "cargo test",
          toolKind: "execute",
          output: "1 failed",
          failed: true,
          at: 2_000,
        },
      ],
      "/Users/me/app",
    );
    const tool = session.blocks.find((block) => block.role === "tool");
    expect(tool?.tool?.title).toBeTruthy();
    expect(tool?.tool?.kind).toBe("execute");
    expect(tool?.tool?.status).toBe("failed");
  });
});
