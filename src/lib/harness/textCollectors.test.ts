import { describe, expect, it } from "vitest";
import {
  claudeTextUpdateFromRecord,
  requireClaudeTextOutput,
} from "./claudeText";
import { codexTextUpdateFromNotification } from "./codexText";
import { textFromCursorTextUpdate } from "./cursorText";

describe("one-shot text collectors", () => {
  it("classifies Codex deltas and completed values", () => {
    expect(
      codexTextUpdateFromNotification("item/agentMessage/delta", {
        itemId: "msg_1",
        delta: "\n\n",
      }),
    ).toEqual({ kind: "delta", scopeId: "msg_1", text: "\n\n" });
    expect(
      codexTextUpdateFromNotification("item/completed", {
        item: { id: "msg_1", type: "agentMessage", text: "complete" },
      }),
    ).toEqual({ kind: "completed", scopeId: "msg_1", text: "complete" });
  });

  it("classifies Claude deltas and completed values", () => {
    expect(
      claudeTextUpdateFromRecord(
        {
          type: "stream_event",
          event: { delta: { type: "text_delta", text: "book" } },
        },
        "record-1",
      ),
    ).toEqual({ kind: "delta", scopeId: "record-1", text: "book" });
    expect(
      claudeTextUpdateFromRecord(
        {
          type: "assistant",
          message: {
            content: [
              { type: "text", text: "book" },
              { type: "text", text: "keeper" },
            ],
          },
        },
        "record-1",
      ),
    ).toEqual({
      kind: "completed",
      scopeId: "record-1",
      text: "bookkeeper",
    });
  });

  it("validates Claude output without trimming exact outer whitespace", () => {
    expect(requireClaudeTextOutput("  bookkeeper\n\n")).toBe(
      "  bookkeeper\n\n",
    );
    expect(() => requireClaudeTextOutput(" \n ")).toThrow(
      "Claude returned empty output",
    );
  });

  it("preserves Cursor ACP chunks and ignores replacement updates", () => {
    expect(
      textFromCursorTextUpdate({
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "  \n" },
        },
      }),
    ).toBe("  \n");
    expect(
      textFromCursorTextUpdate({
        update: {
          sessionUpdate: "agent_message",
          messageId: "message-1",
          content: { type: "text", text: "complete" },
        },
      }),
    ).toBe("");
  });
});
