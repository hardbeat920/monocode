import { describe, expect, it } from "vitest";
import {
  newSession,
  type Session,
} from "../../../features/sessions/model/session";
import { applyHarnessEvent, applyHarnessEvents } from "./apply";
import type { HarnessEvent } from "./types";

function content(session: Session) {
  return {
    ...session,
    blocks: session.blocks.map(({ id: _id, ...block }) => block),
  };
}

function conversation(): Session {
  return {
    ...newSession("codex", "/repo"),
    blocks: [
      { id: "user", role: "user", text: "Help" },
      { id: "reply", role: "assistant", text: "Hello", streaming: true },
    ],
  };
}

describe("batched harness events", () => {
  it("preserves mixed snapshots, repeated tokens, whitespace and message boundaries", () => {
    const events: HarnessEvent[] = [
      { type: "message.delta", text: " " },
      { type: "message.delta", text: "Hello world" },
      { type: "message.delta", text: "Hello world" },
      { type: "message.delta", text: "\n" },
      { type: "message.delta", text: "\n" },
      { type: "status", text: "Working" },
      { type: "message.delta", text: "Next paragraph" },
      { type: "message.delta", text: "." },
      { type: "message.completed" },
      { type: "message.delta", text: "New message" },
      { type: "message.delta", text: "!" },
      { type: "reasoning.delta", text: "" },
      { type: "reasoning.delta", text: "Think" },
      { type: "reasoning.delta", text: "Think carefully" },
      { type: "reasoning.completed" },
      { type: "tool.started", callId: "read", title: "Read" },
      {
        type: "approval.requested",
        callId: "read",
        requestId: 1,
        title: "Read",
      },
      { type: "approval.resolved", requestId: 1, decision: "allow" },
      { type: "tool.updated", callId: "read", status: "completed" },
      { type: "message.delta", text: "Done" },
      { type: "message.delta", text: "." },
    ];
    const session = conversation();
    expect(content(applyHarnessEvents(session, events))).toEqual(
      content(events.reduce(applyHarnessEvent, session)),
    );
    expect(session.blocks[1].text).toBe("Hello");
  });

  it("does not treat combined token fragments as a snapshot of existing text", () => {
    const session = conversation();
    session.blocks[1].text = "abc";
    const events: HarnessEvent[] = [
      { type: "message.delta", text: "a" },
      { type: "message.delta", text: "bc" },
    ];
    expect(applyHarnessEvents(session, events).blocks[1].text).toBe("abcabc");
  });

  it("retains identity for empty reasoning and repeated full snapshots", () => {
    const session = conversation();
    expect(applyHarnessEvents(session, [])).toBe(session);
    expect(
      applyHarnessEvents(session, [
        { type: "reasoning.delta", text: "" },
        { type: "reasoning.delta", text: "" },
      ]),
    ).toBe(session);
    expect(
      applyHarnessEvents(session, [
        { type: "message.delta", text: "Hello" },
        { type: "message.delta", text: "Hello" },
      ]),
    ).toBe(session);
  });

  it("handles ten simultaneous long histories without cloning history blocks", () => {
    const events: HarnessEvent[] = Array.from({ length: 64 }, (_, index) => ({
      type: "message.delta",
      text: ` token-${index}`,
    }));
    for (let thread = 0; thread < 10; thread++) {
      const session = conversation();
      session.blocks = [
        ...Array.from({ length: 2_000 }, (_, index) => ({
          id: `history-${index}`,
          role: "user" as const,
          text: `History ${index}`,
        })),
        ...session.blocks,
      ];
      const next = applyHarnessEvents(session, events);
      expect(next).toEqual(events.reduce(applyHarnessEvent, session));
      for (let index = 0; index < 2_001; index++) {
        expect(next.blocks[index]).toBe(session.blocks[index]);
      }
    }
  });
});
