import { describe, expect, it } from "vitest";
import {
  buildImportedSession,
  claudeImportTarget,
} from "./claudeSessionImport";
import { newSession, type Session } from "./session";

function jsonl(records: Array<Record<string, unknown>>): string {
  return records.map((record) => JSON.stringify(record)).join("\n");
}

function importOf(records: Array<Record<string, unknown>>) {
  return buildImportedSession({
    base: newSession("claude", "/repo", "claude:claude-sonnet-5"),
    transcript: jsonl(records),
    providerSessionId: "conv-1",
  });
}

describe("importing a stored Claude conversation", () => {
  it("rebuilds the exchange as alternating blocks", () => {
    const session = importOf([
      { type: "user", message: { role: "user", content: "neden bozuldu" } },
      {
        type: "assistant",
        message: {
          role: "assistant",
          content: [{ type: "text", text: "sebebi su" }],
        },
      },
      { type: "user", message: { role: "user", content: "peki ya bu" } },
      {
        type: "assistant",
        message: {
          role: "assistant",
          content: [{ type: "text", text: "o da soyle" }],
        },
      },
    ]);

    expect(session.blocks.map((block) => [block.role, block.text])).toEqual([
      ["user", "neden bozuldu"],
      ["assistant", "sebebi su"],
      ["user", "peki ya bu"],
      ["assistant", "o da soyle"],
    ]);
    // Nothing may still look live once the file has been read to the end.
    expect(session.blocks.some((block) => block.streaming)).toBe(false);
  });

  it("binds the conversation so the next turn continues it", () => {
    const session = importOf([
      { type: "user", message: { role: "user", content: "merhaba" } },
    ]);
    expect(session.providerSessionId).toBe("conv-1");
  });

  it("brings tool work across as its own block", () => {
    const session = importOf([
      { type: "user", message: { role: "user", content: "dosyayi oku" } },
      {
        type: "assistant",
        message: {
          role: "assistant",
          content: [
            {
              type: "tool_use",
              id: "t1",
              name: "Read",
              input: { file_path: "/repo/a.ts" },
            },
          ],
        },
      },
      {
        type: "user",
        message: {
          role: "user",
          content: [
            { type: "tool_result", tool_use_id: "t1", content: "icerik" },
          ],
        },
      },
    ]);

    const tool = session.blocks.find((block) => block.role === "tool");
    expect(tool).toBeDefined();
    expect(tool?.text).toContain("Read");
  });

  it("replaces what the thread already showed", () => {
    const base = newSession("claude", "/repo", "claude:claude-sonnet-5");
    const session = buildImportedSession({
      base: {
        ...base,
        blocks: [
          { id: "old-1", role: "user", text: "onceki konusma" },
          { id: "old-2", role: "assistant", text: "onceki cevap" },
        ],
      },
      transcript: jsonl([
        { type: "user", message: { role: "user", content: "yeni soru" } },
      ]),
      providerSessionId: "conv-1",
    });

    // The loaded conversation is the transcript now; keeping the old blocks
    // would show two unrelated conversations back to back.
    expect(session.blocks.map((block) => block.text)).toEqual(["yeni soru"]);
  });

  it("keeps an empty conversation empty", () => {
    const session = importOf([{ type: "mode", mode: "normal" }]);
    expect(session.blocks).toEqual([]);
  });
});

describe("deciding whether an import may still be committed", () => {
  function threadIn(overrides: Partial<Session> = {}): Session {
    return {
      ...newSession("claude", "/repo", "claude:claude-sonnet-5"),
      id: "thread-1",
      ...overrides,
    };
  }

  const target = { sessionId: "thread-1", cwd: "/repo" };

  it("accepts the thread the conversations were listed for", () => {
    const thread = threadIn();
    expect(claudeImportTarget([thread], target)).toBe(thread);
  });

  it("refuses a thread that has been closed", () => {
    expect(claudeImportTarget([], target)).toBeNull();
  });

  it("refuses a thread that started a turn", () => {
    expect(claudeImportTarget([threadIn({ busy: true })], target)).toBeNull();
  });

  it("refuses a thread that moved to another harness", () => {
    expect(claudeImportTarget([threadIn({ harness: "codex" })], target)).toBe(
      null,
    );
  });

  it("refuses a thread that moved to another account", () => {
    expect(
      claudeImportTarget([threadIn({ providerAccountId: "work" })], target),
    ).toBeNull();
    // And the other way: picked under an account, since cleared.
    expect(
      claudeImportTarget([threadIn()], {
        ...target,
        providerAccountId: "work",
      }),
    ).toBeNull();
  });

  it("refuses a thread that moved to another working copy", () => {
    // The conversations were listed for /repo. Claude drops a resume binding
    // whose directory is not the one the next turn runs in, so binding one of
    // them here would start a new conversation instead of continuing it.
    expect(
      claudeImportTarget([threadIn({ worktreeCwd: "/repo/.tree/a" })], target),
    ).toBeNull();
  });

  it("accepts the worktree the conversations were listed for", () => {
    const thread = threadIn({ worktreeCwd: "/repo/.tree/a" });
    expect(
      claudeImportTarget([thread], { ...target, cwd: "/repo/.tree/a" }),
    ).toBe(thread);
  });
});
