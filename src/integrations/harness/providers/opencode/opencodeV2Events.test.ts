import { describe, expect, it } from "vitest";
import {
  OpenCodeV2EventTranslator,
  OpenCodeV2InboxTracker,
} from "./opencodeV2Events";

function event(type: string, data: Record<string, unknown>) {
  return {
    type,
    properties: {
      sessionID: "ses_1",
      assistantMessageID: "msg_1",
      ordinal: 0,
      ...data,
    },
  };
}

describe("OpenCodeV2EventTranslator text streaming", () => {
  it("forwards whitespace-only deltas for text and reasoning", () => {
    const translator = new OpenCodeV2EventTranslator();

    translator.translate(event("session.text.started", {}));
    expect(
      translator.translate(event("session.text.delta", { delta: "  \n" })),
    ).toMatchObject({
      type: "message.part.delta",
      properties: { partID: "msg_1:text:0", delta: "  \n" },
    });

    translator.translate(event("session.reasoning.started", {}));
    expect(
      translator.translate(event("session.reasoning.delta", { delta: "\n\n" })),
    ).toMatchObject({
      type: "message.part.delta",
      properties: { partID: "msg_1:reasoning:0", delta: "\n\n" },
    });
  });

  it("drops only empty or non-string deltas", () => {
    const translator = new OpenCodeV2EventTranslator();
    translator.translate(event("session.text.started", {}));

    expect(
      translator.translate(event("session.text.delta", { delta: "" })),
    ).toBeNull();
    expect(
      translator.translate(event("session.text.delta", { delta: 5 })),
    ).toBeNull();
  });
});

describe("OpenCodeV2InboxTracker", () => {
  it("does not settle a queued prompt when a compaction end has no id", () => {
    const tracker = new OpenCodeV2InboxTracker();
    tracker.observe("session.inbox.delivered", { inboxID: "msg_compact" });
    tracker.observe("session.inbox.delivered", { inboxID: "msg_prompt" });
    tracker.observe("session.compaction.started", { reason: "manual" });
    tracker.observe("session.compaction.ended", {});
    expect(tracker.outcome("msg_prompt")).toBeUndefined();

    tracker.observe("session.idle", { execution: "succeeded" });
    expect(tracker.outcome("msg_compact")).toEqual({ kind: "succeeded" });
    expect(tracker.outcome("msg_prompt")).toEqual({ kind: "succeeded" });
  });
});
