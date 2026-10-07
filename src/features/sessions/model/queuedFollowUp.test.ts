import { describe, expect, it, vi } from "vitest";
import { TurnNotReadyError } from "../../../integrations/harness/core/types";
import { deliverQueuedFollowUp } from "./queuedFollowUp";
import { dequeueQueuedMessage } from "./messageQueue";
import { newSession, type Session } from "./session";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

function delivery() {
  let session: Session = {
    ...newSession("codex", "/tmp"),
    busy: true,
    turnReady: true,
    queueStatus: "active",
    queuedMessages: [
      { id: "first", text: "Use TypeScript", attachments: [] },
      { id: "second", text: "Also add a README", attachments: [] },
    ],
  };
  let currentTurn = true;
  const message = session.queuedMessages![0];
  const options = {
    message,
    current: () => session,
    isCurrentTurn: () => currentTurn,
    prepare: vi.fn(async () => message.text),
    steer: vi.fn(async (_text: string) => {}),
    delivered: vi.fn(() => {
      session = dequeueQueuedMessage(session, message.id);
    }),
    deferred: vi.fn(() => {
      session = { ...session, turnReady: false };
    }),
    failed: vi.fn((error: string) => {
      session = {
        ...session,
        queueStatus: "paused",
        queuedMessages: session.queuedMessages?.map((entry) =>
          entry.id === message.id ? { ...entry, error } : entry,
        ),
      };
    }),
  };
  return {
    options,
    session: () => session,
    patch: (patch: Partial<Session>) => {
      session = { ...session, ...patch };
    },
    stop: () => {
      currentTurn = false;
      session = { ...session, busy: false };
    },
  };
}

describe("pending Mono follow-up delivery", () => {
  it("waits for cold startup and never lets a later message overtake the head", async () => {
    const d = delivery();
    d.patch({ turnReady: false });
    expect(await deliverQueuedFollowUp(d.options)).toBe(false);
    expect(d.options.prepare).not.toHaveBeenCalled();
    d.patch({ turnReady: true });
    expect(
      await deliverQueuedFollowUp({
        ...d.options,
        message: d.session().queuedMessages![1],
      }),
    ).toBe(false);
    expect(d.options.steer).not.toHaveBeenCalled();
    expect(await deliverQueuedFollowUp(d.options)).toBe(true);
    expect(d.options.steer).toHaveBeenCalledWith("Use TypeScript");
    expect(d.session().queuedMessages?.map((message) => message.id)).toEqual([
      "second",
    ]);
  });

  it("keeps the message pending until the provider acknowledges delivery", async () => {
    const d = delivery();
    const accepted = deferred<void>();
    d.options.steer.mockImplementation(() => accepted.promise);
    const sending = deliverQueuedFollowUp(d.options);
    await Promise.resolve();
    expect(d.session().queuedMessages?.[0]).toBe(d.options.message);
    expect(d.options.delivered).not.toHaveBeenCalled();
    accepted.resolve();
    expect(await sending).toBe(true);
    expect(d.session().busy).toBe(true);
  });

  it.each(["edit", "delete", "stop", "question", "switch"])(
    "honors a %s while attachments or prompt context are being prepared",
    async (action) => {
      const d = delivery();
      const prepared = deferred<string>();
      d.options.prepare.mockImplementation(() => prepared.promise);
      const sending = deliverQueuedFollowUp(d.options);
      if (action === "edit")
        d.patch({ queuedMessages: [{ ...d.options.message, text: "Edited" }] });
      if (action === "delete") d.patch({ queuedMessages: [] });
      if (action === "stop") d.stop();
      if (action === "question")
        d.patch({ pendingQuestion: { requestId: 1, questions: [] } });
      if (action === "switch")
        d.patch({
          pendingSwitch: {
            from: "codex",
            fromModel: "model",
            fromSettings: {},
          },
        });
      prepared.resolve("prepared");
      expect(await sending).toBe(false);
      expect(d.options.steer).not.toHaveBeenCalled();
      expect(d.options.delivered).not.toHaveBeenCalled();
    },
  );

  it.each(["preparation", "provider"])(
    "retains a failed %s without settling the original turn",
    async (failure) => {
      const d = delivery();
      (failure === "preparation"
        ? d.options.prepare
        : d.options.steer
      ).mockRejectedValueOnce(new Error("Connection lost"));
      expect(await deliverQueuedFollowUp(d.options)).toBe(false);
      expect(d.session()).toMatchObject({ busy: true, queueStatus: "paused" });
      expect(d.session().queuedMessages?.[0]).toMatchObject({
        id: "first",
        error: "Connection lost",
      });
      expect(d.options.delivered).not.toHaveBeenCalled();
    },
  );

  it("waits for a new turn when the provider finished during delivery", async () => {
    const d = delivery();
    d.options.steer.mockRejectedValueOnce(new TurnNotReadyError("Turn ended"));
    expect(await deliverQueuedFollowUp(d.options)).toBe(false);
    expect(d.session()).toMatchObject({
      turnReady: false,
      queueStatus: "active",
    });
    expect(d.session().queuedMessages?.[0]).toBe(d.options.message);
    expect(d.options.failed).not.toHaveBeenCalled();
  });
});
