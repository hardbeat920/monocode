import { describe, expect, it } from "vitest";
import { newSession, type Block } from "../../sessions/model/session";
import { sessionConversationPage } from "./sessionConversation";

function conversation(blocks: Block[]) {
  return { ...newSession("codex", "/repo"), id: "target", blocks };
}

describe("sessionConversationPage", () => {
  it.each([undefined, 3])(
    "keeps an app completion report separate from the preceding user exchange with sessionCount=%s",
    (sessionCount) => {
      const session = conversation([
        { id: "u", role: "user", text: "Explain the design" },
        { id: "a", role: "assistant", text: "It uses a queue." },
        {
          id: "notification",
          role: "user",
          text: "Hidden app instructions",
          internal: true,
          monoSessionCompletion: {
            sessionId: "worker",
            title: "API fix",
            status: "completed",
            ...(sessionCount ? { sessionCount } : {}),
          },
        },
        { id: "report", role: "assistant", text: "The API fix passed tests." },
      ]);
      const page = sessionConversationPage(session);
      expect(page.turns).toHaveLength(2);
      expect(page.turns[0].assistant?.text).toBe("It uses a queue.");
      expect(page.turns[1].user.text).toBe(
        sessionCount
          ? "MonoCode: results from 3 sessions"
          : "MonoCode: session completed: API fix",
      );
      expect(page.turns[1].assistant?.text).toBe("The API fix passed tests.");
      expect(JSON.stringify(page)).not.toContain("Hidden app instructions");
    },
  );

  it("returns the newest three exchanges without tools or reasoning, then older pages", () => {
    const blocks: Block[] = [];
    for (let i = 1; i <= 5; i += 1) {
      blocks.push({ id: `u${i}`, role: "user", text: `Question ${i}` });
      blocks.push({ id: `t${i}`, role: "tool", text: "secret tool output" });
      blocks.push({
        id: `r${i}`,
        role: "reasoning",
        text: "private reasoning",
      });
      blocks.push({ id: `a${i}`, role: "assistant", text: `Answer ${i}` });
    }
    const session = conversation(blocks);
    const recent = sessionConversationPage(session);
    expect(recent.turns.map((turn) => turn.turnId)).toEqual(["u3", "u4", "u5"]);
    expect(recent.nextBefore).toBe("u3");
    expect(JSON.stringify(recent)).not.toMatch(
      /secret tool output|private reasoning/,
    );
    const older = sessionConversationPage(session, {
      before: recent.nextBefore!,
      limit: 2,
    });
    expect(older.turns.map((turn) => turn.turnId)).toEqual(["u1", "u2"]);
    expect(older.nextBefore).toBeNull();
  });

  it("caps individual messages and lets a caller request a larger excerpt", () => {
    const session = conversation([
      { id: "u", role: "user", text: "U".repeat(7000) },
      { id: "a", role: "assistant", text: "A".repeat(7000) },
    ]);
    expect(sessionConversationPage(session).turns[0].assistant).toEqual({
      text: "A".repeat(1200),
      truncated: true,
    });
    expect(
      sessionConversationPage(session, { maxChars: 6000 }).turns[0].user.text,
    ).toHaveLength(6000);
    expect(() => sessionConversationPage(session, { limit: 4 })).toThrow(
      "limit",
    );
    expect(() =>
      sessionConversationPage(session, { before: "missing" }),
    ).toThrow("before");
  });

  it("omits drafts and shows the latest assistant response in each exchange", () => {
    const session = conversation([
      { id: "u", role: "user", text: "/monocode list notes" },
      { id: "a1", role: "assistant", text: "Working" },
      { id: "a2", role: "assistant", text: "Done" },
      { id: "draft", role: "user", text: "unsent", draft: true },
    ]);
    const page = sessionConversationPage(session);
    expect(page.hasDraft).toBe(true);
    expect(page.turns).toEqual([
      {
        turnId: "u",
        user: { text: "list notes", truncated: false },
        assistant: { text: "Done", truncated: false },
        earlierAssistantMessages: 1,
      },
    ]);
  });
});
