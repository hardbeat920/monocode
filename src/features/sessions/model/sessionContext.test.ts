import { describe, expect, it, vi } from "vitest";
import { newSession, type Block, type Session } from "./session";
import {
  SESSION_CONTEXT_LEAD,
  appendAttachedContext,
  expandSessionContext,
  parseAttachedContext,
  withSessionContextCard,
  withoutSessionContextCard,
  type SessionContextStorage,
} from "./sessionContext";

function source(blocks: Block[], title = "Fix login"): Session {
  return { ...newSession("codex", "/repo"), id: "src-1", title, blocks };
}

function storage(): SessionContextStorage & {
  saveSnapshot: ReturnType<typeof vi.fn>;
} {
  return {
    snapshotAssets: vi.fn(async () => []),
    saveSnapshot: vi.fn(async (sessionId: string, id: string) => `/data/context-history/${sessionId}/${id}.md`),
  };
}

describe("session context cards", () => {
  it("adds each session once and never to its own composer", () => {
    const one = withSessionContextCard(undefined, { id: "a", title: " A " }, "target");
    expect(one).toEqual([{ id: "a", title: "A" }]);
    expect(withSessionContextCard(one, { id: "a", title: "A" }, "target")).toBe(one);
    expect(withSessionContextCard(one, { id: "target", title: "Self" }, "target")).toBe(one);
    expect(withoutSessionContextCard(one, "a")).toBeUndefined();
  });
});

describe("expandSessionContext", () => {
  it("sends user and assistant messages with a retrieval path, and leaves tools and reasoning out", async () => {
    const store = storage();
    const text = await expandSessionContext(
      "Compare with this",
      [{ id: "src-1", title: "Stale title" }],
      async () =>
        source([
          { id: "u1", role: "user", text: "Fix the login bug" },
          { id: "t1", role: "tool", text: "secret tool output", tool: { kind: "shell", status: "completed" } },
          { id: "r1", role: "reasoning", text: "private reasoning" },
          { id: "a1", role: "assistant", text: "Fixed in auth.ts" },
        ]),
      store,
    );
    expect(text.startsWith("Compare with this\n\n<attached_context>")).toBe(true);
    expect(text).toContain('<session id="src-1" title="Fix login">');
    expect(text).toContain("Fix the login bug");
    expect(text).toContain("Fixed in auth.ts");
    expect(text).not.toMatch(/secret tool output|private reasoning/);
    expect(text).toContain('"retrievalPath":"/data/context-history/src-1/');
    // The saved snapshot is the full transcript, tool records included.
    expect(store.saveSnapshot.mock.calls[0][2]).toContain("secret tool output");
  });

  it("keeps the newest messages inside the byte budget", async () => {
    const blocks: Block[] = [];
    for (let i = 1; i <= 40; i += 1) {
      blocks.push({ id: `u${i}`, role: "user", text: `Question ${i} ${"x".repeat(600)}` });
      blocks.push({ id: `a${i}`, role: "assistant", text: `Answer ${i}` });
    }
    const text = await expandSessionContext("", [{ id: "src-1", title: "T" }], async () => source(blocks), storage());
    expect(text.startsWith(SESSION_CONTEXT_LEAD)).toBe(true);
    expect(new TextEncoder().encode(text).byteLength).toBeLessThan(24_000);
    expect(text).toContain("Question 40");
    expect(text).not.toContain("Question 20 ");
    expect(text).toContain('"budget"');
  });

  it("still sends the block when the source is gone or the snapshot fails", async () => {
    const store = storage();
    store.saveSnapshot.mockRejectedValue(new Error("disk full"));
    const text = await expandSessionContext(
      "Hi",
      [
        { id: "gone", title: "Old" },
        { id: "src-1", title: "T" },
      ],
      async (id) => (id === "gone" ? null : source([{ id: "u", role: "user", text: "Q" }])),
      store,
    );
    expect(text).toContain('<session id="gone" title="Old">\n(This session is no longer available.)');
    expect(text).not.toContain('"retrievalPath"');
  });
});

describe("parseAttachedContext", () => {
  it("turns an expanded prompt back into the user's words and chips", () => {
    const sent = appendAttachedContext("Look at this", [
      { id: "a-1", title: 'Fix "login" <now>', context: null },
      { id: "b-2", title: "", context: null },
    ]);
    expect(parseAttachedContext(sent)).toEqual({
      text: "Look at this",
      sessions: [
        { id: "a-1", title: 'Fix "login" <now>' },
        { id: "b-2", title: "Untitled session" },
      ],
    });
    const bare = appendAttachedContext("", [{ id: "a-1", title: "A", context: null }]);
    expect(parseAttachedContext(bare).text).toBe("");
  });

  it("leaves ordinary text alone", () => {
    expect(parseAttachedContext("plain")).toEqual({ text: "plain", sessions: [] });
  });
});
