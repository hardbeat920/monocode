import { describe, expect, it } from "vitest";
import { appendPreparingHandoff } from "./handoff";
import {
  canDispatchQueuedHead,
  dequeueQueuedMessage,
  isEditingQueuedHead,
  queuedHead,
  queuedMessageForSubmit,
} from "./messageQueue";
import { newSession, type QueuedMessage, type Session } from "./session";

function queued(id: string, text = id): QueuedMessage {
  return { id, text, attachments: [] };
}

function chat(patch: Partial<Session> = {}): Session {
  return {
    ...newSession("claude", "/tmp/project"),
    queuedMessages: [queued("a", "first"), queued("b", "second")],
    queueStatus: "active",
    ...patch,
  };
}

describe("queuedHead", () => {
  it("returns the first queued follow-up", () => {
    expect(queuedHead(chat())?.id).toBe("a");
    expect(queuedHead(chat({ queuedMessages: undefined }))).toBeUndefined();
  });
});

describe("isEditingQueuedHead", () => {
  it("is true only when the head row is the one being edited", () => {
    expect(isEditingQueuedHead(chat())).toBe(false);
    expect(
      isEditingQueuedHead(chat({ editingQueuedMessageId: "a" })),
    ).toBe(true);
    expect(
      isEditingQueuedHead(chat({ editingQueuedMessageId: "b" })),
    ).toBe(false);
  });
});

describe("canDispatchQueuedHead", () => {
  it("dispatches an idle session with a queued head", () => {
    expect(canDispatchQueuedHead(chat())).toBe(true);
  });

  it("holds while the session is busy, paused, or resuming", () => {
    expect(canDispatchQueuedHead(chat({ busy: true }))).toBe(false);
    expect(canDispatchQueuedHead(chat({ queueStatus: "paused" }))).toBe(false);
    expect(canDispatchQueuedHead(chat({ queueStatus: "resuming" }))).toBe(
      false,
    );
  });

  it("holds only when the head item is being edited", () => {
    expect(
      canDispatchQueuedHead(chat({ editingQueuedMessageId: "a" })),
    ).toBe(false);
    expect(
      canDispatchQueuedHead(chat({ editingQueuedMessageId: "b" })),
    ).toBe(true);
  });

  it("does not dispatch during a preparing handoff", () => {
    const preparing = appendPreparingHandoff(
      chat({ queuedMessages: [queued("a")] }),
      "claude",
      "cursor",
    );
    expect(canDispatchQueuedHead(preparing)).toBe(false);
  });

  it("does not dispatch an empty queue", () => {
    expect(
      canDispatchQueuedHead(chat({ queuedMessages: undefined })),
    ).toBe(false);
  });
});

describe("dequeueQueuedMessage", () => {
  it("drops the id and clears queue state when the last item goes", () => {
    const one = chat({ queuedMessages: [queued("a")], queueStatus: "active" });
    expect(dequeueQueuedMessage(one, "a")).toMatchObject({
      queuedMessages: undefined,
      queueStatus: undefined,
    });
  });

  it("keeps editing another row after the head is sent", () => {
    const next = dequeueQueuedMessage(
      chat({ editingQueuedMessageId: "b" }),
      "a",
    );
    expect(next.queuedMessages?.map((message) => message.id)).toEqual(["b"]);
    expect(next.editingQueuedMessageId).toBe("b");
    expect(next.queueStatus).toBe("active");
  });
});

describe("queuedMessageForSubmit", () => {
  it("only auto-dispatches the idle head", () => {
    expect(queuedMessageForSubmit(chat(), "a", "dispatch")?.id).toBe("a");
    expect(queuedMessageForSubmit(chat(), "b", "dispatch")).toBeUndefined();
    expect(
      queuedMessageForSubmit(chat({ busy: true }), "a", "dispatch"),
    ).toBeUndefined();
  });

  it("lets Steer target any remaining row, including while busy or paused", () => {
    expect(
      queuedMessageForSubmit(chat({ busy: true }), "b", "steer")?.id,
    ).toBe("b");
    expect(
      queuedMessageForSubmit(chat({ queueStatus: "paused" }), "a", "steer")?.id,
    ).toBe("a");
    expect(queuedMessageForSubmit(chat(), "missing", "steer")).toBeUndefined();
  });
});
