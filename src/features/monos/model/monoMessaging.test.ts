import { describe, expect, it } from "vitest";
import {
  appendUser,
  applyHarnessEvent,
  stopStreaming,
} from "../../../integrations/harness/core/apply";
import { newSession, type QueuedMessage } from "../../sessions/model/session";
import {
  acknowledgeMonoMessage,
  enqueueMonoMessage,
  monoMessageDeliveries,
  monoPendingTranscriptBlocks,
} from "./monoMessaging";

function message(id: string): QueuedMessage {
  return { id, text: id, attachments: [] };
}

describe("optimistic Mono messages", () => {
  it("shows older saved outboxes as stable bubbles and recovers pending image previews", () => {
    const session = {
      ...newSession("codex", "/tmp"),
      queuedMessages: [message("legacy")],
    };
    const preview = monoPendingTranscriptBlocks(session, session.blocks);
    expect(preview[0]).toMatchObject({
      id: "legacy",
      role: "user",
      text: "legacy",
    });
    expect(monoPendingTranscriptBlocks(session, session.blocks)[0]).toBe(
      preview[0],
    );
    expect(session.blocks).toHaveLength(0);
    const accepted = acknowledgeMonoMessage(
      session,
      session.queuedMessages[0],
      { mode: "new-turn" },
    );
    expect(monoPendingTranscriptBlocks(accepted, accepted.blocks)).toBe(
      accepted.blocks,
    );
    expect(accepted.blocks[0].id).toBe(preview[0].id);

    const image = {
      id: "photo",
      name: "Paste.png",
      mimeType: "image/png",
      kind: "image" as const,
      size: 12,
      data: "bytes",
    };
    const pending = enqueueMonoMessage(newSession("codex", "/tmp"), {
      ...message("image"),
      attachments: [image],
    });
    const saved = [
      { ...pending.blocks[0], attachments: [{ ...image, data: undefined }] },
    ];
    expect(
      monoPendingTranscriptBlocks(pending, saved)[0].attachments?.[0].data,
    ).toBe("bytes");
    expect(saved[0].attachments[0].data).toBeUndefined();
  });
  it("shows rapid follow-ups immediately, in order, while keeping the current turn and stream active", () => {
    const first = appendUser(newSession("codex", "/tmp"), "First");
    const started = {
      ...first,
      turnReady: true,
      blocks: [
        ...first.blocks,
        {
          id: "reply",
          role: "assistant" as const,
          text: "Working",
          streaming: true,
        },
      ],
    };
    const session = enqueueMonoMessage(
      enqueueMonoMessage(started, message("second")),
      message("third"),
    );
    expect(session.blocks.map((block) => block.text)).toEqual([
      "First",
      "Working",
      "second",
      "third",
    ]);
    expect(session.blocks[1]).toBe(started.blocks[1]);
    expect(session.blocks[1].streaming).toBe(true);
    expect(session.busy).toBe(true);
    expect(session.turnReady).toBe(true);
    expect(session.queuedMessages?.map((message) => message.blockId)).toEqual([
      "second",
      "third",
    ]);
  });

  it("acknowledges a follow-up without duplicating or moving its bubble", () => {
    let session = enqueueMonoMessage(
      appendUser(newSession("codex", "/tmp"), "First"),
      message("second"),
    );
    session = enqueueMonoMessage(session, message("third"));
    const ids = session.blocks.map((block) => block.id);
    const accepted = acknowledgeMonoMessage(
      session,
      session.queuedMessages![0],
      { mode: "follow-up" },
    );
    expect(accepted.blocks.map((block) => block.id)).toEqual(ids);
    expect(accepted.blocks[1].sentAt).toBe(session.blocks[1].sentAt);
    expect(accepted.queuedMessages?.map((message) => message.id)).toEqual([
      "third",
    ]);
    expect(monoMessageDeliveries(accepted).has("second")).toBe(false);
    expect(monoMessageDeliveries(accepted).get("third")?.status).toBe(
      "pending",
    );
  });

  it("promotes the existing bubble when a provider needs a new turn", () => {
    const session = enqueueMonoMessage(
      stopStreaming(appendUser(newSession("fx", "/tmp"), "First")),
      message("second"),
    );
    expect(session.busy).toBe(false);
    const accepted = acknowledgeMonoMessage(
      session,
      session.queuedMessages![0],
      { mode: "new-turn" },
    );
    expect(accepted.blocks.map((block) => block.id)).toEqual(
      session.blocks.map((block) => block.id),
    );
    expect(accepted.blocks[1].startedAt).toBeTypeOf("number");
    expect(accepted.blocks[1].sentAt).toBe(session.blocks[1].sentAt);
    expect(accepted.busy).toBe(true);
    expect(accepted.turnReady).toBe(false);
    expect(accepted.queuedMessages).toBeUndefined();
  });

  it("never resurrects a finished turn when a follow-up acknowledgement arrives late", () => {
    const session = enqueueMonoMessage(
      appendUser(newSession("codex", "/tmp"), "First"),
      message("second"),
    );
    const accepted = acknowledgeMonoMessage(
      stopStreaming(session),
      session.queuedMessages![0],
      { mode: "follow-up" },
    );
    expect(accepted.busy).toBe(false);
    expect(
      accepted.blocks.filter((block) => block.id === "second"),
    ).toHaveLength(1);
  });

  it("keeps provider turn identity on the accepted message while newer bubbles are still pending", () => {
    const session = enqueueMonoMessage(
      appendUser(newSession("codex", "/tmp"), "First"),
      message("second"),
    );
    const ready = applyHarnessEvent(session, {
      type: "turn.started",
      providerTurnId: "native-turn",
    });
    expect(ready.blocks[0].providerTurnId).toBe("native-turn");
    expect(ready.blocks[1].providerTurnId).toBeUndefined();
  });

  it("keeps failed bubbles available for retry and preserves a paused outbox when more messages arrive", () => {
    const session = enqueueMonoMessage(
      {
        ...newSession("codex", "/tmp"),
        queueStatus: "paused",
        queuedMessages: [
          { ...message("failed"), blockId: "failed", error: "Offline" },
        ],
      },
      message("next"),
    );
    expect(session.queueStatus).toBe("paused");
    expect(monoMessageDeliveries(session).get("failed")).toEqual({
      status: "failed",
      error: "Offline",
    });
    expect(monoMessageDeliveries(session).get("next")).toEqual({
      status: "paused",
    });
  });

  it("retains attachments on the optimistic bubble and acknowledges older saved queues once", () => {
    const file = {
      id: "image",
      name: "Paste.png",
      mimeType: "image/png",
      kind: "image" as const,
      size: 12,
      data: "bytes",
    };
    const session = enqueueMonoMessage(newSession("codex", "/tmp"), {
      ...message("image-message"),
      attachments: [file],
    });
    expect(session.blocks[0].attachments?.[0].data).toBe("bytes");
    const legacy = {
      ...newSession("codex", "/tmp"),
      queuedMessages: [message("legacy")],
    };
    const accepted = acknowledgeMonoMessage(legacy, legacy.queuedMessages[0], {
      mode: "new-turn",
    });
    expect(accepted.blocks).toHaveLength(1);
    expect(accepted.blocks[0].text).toBe("legacy");
  });
});
