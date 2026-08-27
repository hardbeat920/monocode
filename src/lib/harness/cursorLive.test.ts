import { describe, expect, it } from "vitest";
import { textEventFromCursorUpdate } from "./cursor";

describe("Cursor ACP v1 text updates", () => {
  it("preserves exact message chunks", () => {
    expect(
      textEventFromCursorUpdate({
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "\n\n" },
        },
      }),
    ).toEqual({ type: "message.delta", text: "\n\n" });
  });

  it("does not append unproven whole-message replacement updates", () => {
    expect(
      textEventFromCursorUpdate({
        update: {
          sessionUpdate: "agent_message",
          messageId: "message-1",
          content: [
            { type: "text", text: "First" },
            { type: "text", text: "Second" },
          ],
        },
      }),
    ).toBeNull();
  });

  it("maps thought chunks without trimming", () => {
    expect(
      textEventFromCursorUpdate({
        update: {
          sessionUpdate: "agent_thought_chunk",
          content: { type: "text", text: "  " },
        },
      }),
    ).toEqual({ type: "reasoning.delta", text: "  " });
  });
});
