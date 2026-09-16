import { describe, expect, it } from "vitest";
import {
  canEditLastTurn,
  lastTurnRecall,
  lastUserTurnStartIndex,
  truncateBeforeLastUserTurn,
} from "./editLastTurn";
import { newSession, type Block } from "./session";

function chat(blocks: Block[]) {
  return { ...newSession("pi", "/tmp"), blocks };
}

describe("editLastTurn", () => {
  it("finds the latest user turn", () => {
    const blocks: Block[] = [
      { id: "u1", role: "user", text: "first" },
      { id: "a1", role: "assistant", text: "ok" },
      { id: "u2", role: "user", text: "second" },
      { id: "a2", role: "assistant", text: "done" },
    ];
    expect(lastUserTurnStartIndex(blocks)).toBe(2);
    expect(truncateBeforeLastUserTurn(blocks).map((block) => block.id)).toEqual([
      "u1",
      "a1",
    ]);
  });

  it("recalls the last user message", () => {
    const session = chat([
      { id: "u1", role: "user", text: "hello" },
      { id: "a1", role: "assistant", text: "hi" },
    ]);
    expect(lastTurnRecall(session)).toEqual({
      text: "hello",
      attachments: [],
    });
  });

  it("allows edit on idle pi sessions without queued follow-ups", () => {
    const session = chat([
      { id: "u1", role: "user", text: "hello" },
      { id: "a1", role: "assistant", text: "hi" },
    ]);
    expect(canEditLastTurn(session)).toBe(true);
  });

  it("allows edit on idle OpenCode sessions", () => {
    const session = {
      ...chat([
        { id: "u1", role: "user", text: "hello" },
        { id: "a1", role: "assistant", text: "hi" },
      ]),
      harness: "opencode" as const,
    };
    expect(canEditLastTurn(session)).toBe(true);
  });

  it("blocks edit while busy, queued, or on unsupported harnesses", () => {
    const base = chat([
      { id: "u1", role: "user", text: "hello" },
      { id: "a1", role: "assistant", text: "hi" },
    ]);
    expect(canEditLastTurn({ ...base, busy: true })).toBe(false);
    expect(
      canEditLastTurn({
        ...base,
        queuedMessages: [
          { id: "q1", text: "next", attachments: [] },
        ],
      }),
    ).toBe(false);
    expect(canEditLastTurn({ ...chat([]), harness: "claude" })).toBe(false);
  });
});
