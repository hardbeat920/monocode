import { describe, expect, it } from "vitest";
import {
  buildImportedSession,
  claudeImportTarget,
  type ClaudeImportTarget,
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

  const target = { sessionId: "thread-1", cwd: "/repo", turnGen: 3 };

  /** The check as the import runs it: against the target captured at the pick. */
  function check(
    sessions: Session[],
    overrides: Partial<ClaudeImportTarget> = {},
    turnGen = target.turnGen,
  ) {
    return claudeImportTarget(sessions, { ...target, ...overrides }, turnGen);
  }

  it("accepts the thread the conversations were listed for", () => {
    const thread = threadIn();
    expect(check([thread])).toBe(thread);
  });

  it("refuses a thread that has been closed", () => {
    expect(check([])).toBeNull();
  });

  it("refuses a thread that started a turn", () => {
    expect(check([threadIn({ busy: true })])).toBeNull();
  });

  it("refuses a thread that moved to another harness", () => {
    expect(check([threadIn({ harness: "codex" })])).toBeNull();
  });

  it("refuses a thread that moved to another account", () => {
    expect(check([threadIn({ providerAccountId: "work" })])).toBeNull();
    // And the other way: picked under an account, since cleared.
    expect(check([threadIn()], { providerAccountId: "work" })).toBeNull();
  });

  it("refuses a thread that moved to another working copy", () => {
    // The conversations were listed for /repo. Claude drops a resume binding
    // whose directory is not the one the next turn runs in, so binding one of
    // them here would start a new conversation instead of continuing it.
    expect(check([threadIn({ worktreeCwd: "/repo/.tree/a" })])).toBeNull();
  });

  it("accepts the worktree the conversations were listed for", () => {
    const thread = threadIn({ worktreeCwd: "/repo/.tree/a" });
    expect(check([thread], { cwd: "/repo/.tree/a" })).toBe(thread);
  });

  it("refuses a thread whose turn both started and finished during the read", () => {
    // The thread was idle when the conversation was picked and is idle again
    // now, so `busy` reads the same at both ends and sees nothing. The turn in
    // between left an answer that replacing the transcript would throw away.
    const settled = threadIn({
      busy: false,
      blocks: [
        { id: "b1", role: "user", text: "arada sordum" },
        { id: "b2", role: "assistant", text: "arada cevapladim" },
      ],
    });
    expect(check([settled], {}, target.turnGen + 1)).toBeNull();
  });

  it("refuses a thread whose turn was cancelled during the read", () => {
    // Cancelling advances the generation too, and leaves the partial reply.
    expect(check([threadIn()], {}, target.turnGen + 1)).toBeNull();
  });

  it("accepts a thread no turn has run on since the pick", () => {
    const thread = threadIn();
    expect(check([thread], {}, target.turnGen)).toBe(thread);
  });
});
