import { afterEach, describe, expect, it } from "vitest";
import { appendPreparingHandoff } from "./handoff";
import {
  canDispatchQueuedHead,
  dequeueQueuedMessage,
  isEditingQueuedHead,
  queuedHead,
  queuedMessageForSubmit,
} from "./messageQueue";
import {
  newSession,
  type ModelTarget,
  type QueuedMessage,
  type Session,
} from "./session";
import { resetHarnessModelOverlays, setHarnessModels } from "./models";

afterEach(resetHarnessModelOverlays);

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
    expect(isEditingQueuedHead(chat({ editingQueuedMessageId: "a" }))).toBe(
      true,
    );
    expect(isEditingQueuedHead(chat({ editingQueuedMessageId: "b" }))).toBe(
      false,
    );
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

  it("holds while the last turn is stopped at a usage limit", () => {
    expect(
      canDispatchQueuedHead(chat({ usageLimit: { resetsAt: 1_000 } })),
    ).toBe(false);
  });

  it("requires inspection even if a submitted request's queue is resumed explicitly", () => {
    const session = chat({
      providerContext: {
        version: 1,
        bindings: [],
        delivery: {
          switchId: "switch",
          status: "uncertain",
          mode: "inline",
          from: "codex",
          to: "claude",
          cwd: "/tmp/project",
          currentUserBlockId: "user",
          includedBlockIds: [],
          omittedBlockIds: [],
          requestSubmitted: true,
          needsInspection: true,
        },
      },
    });
    expect(canDispatchQueuedHead(session)).toBe(false);
    expect(queuedMessageForSubmit(session, "a", "dispatch")).toBeUndefined();
    expect(queuedMessageForSubmit(session, "a", "steer")).toBeUndefined();
  });

  it("holds only when the head item is being edited", () => {
    expect(canDispatchQueuedHead(chat({ editingQueuedMessageId: "a" }))).toBe(
      false,
    );
    expect(canDispatchQueuedHead(chat({ editingQueuedMessageId: "b" }))).toBe(
      true,
    );
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
    expect(canDispatchQueuedHead(chat({ queuedMessages: undefined }))).toBe(
      false,
    );
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
  const active: ModelTarget = {
    harness: "codex",
    model: "codex:active",
    modelSettings: { reasoningEffort: "high", serviceTier: "fast" },
  };

  it.each([
    { ...active, model: "codex:next" },
    {
      ...active,
      modelSettings: { ...active.modelSettings, reasoningEffort: "low" },
    },
    { ...active, harness: "claude" as const, model: "claude:sonnet-5" },
  ])(
    "keeps an incompatible saved selection queued for its idle turn",
    (selection) => {
      const row = { ...queued("a", "Use the saved selection"), selection };
      const session = chat({
        ...active,
        busy: true,
        providerAccountId: "source-account",
        queuedMessages: [row],
      });
      const snapshot = structuredClone(session);
      expect(
        queuedMessageForSubmit(session, row.id, "steer", active),
      ).toBeUndefined();
      expect(session).toEqual(snapshot);
      expect(session.queuedMessages?.[0]).toBe(row);
      expect(
        queuedMessageForSubmit(
          { ...session, busy: false },
          row.id,
          "dispatch",
          active,
        ),
      ).toBe(row);
    },
  );

  it("allows a matching saved selection to steer the active turn after the picker changes", () => {
    const row = {
      ...queued("b"),
      selection: {
        ...active,
        modelSettings: { serviceTier: "fast", reasoningEffort: "high" },
      },
    };
    const session = chat({
      harness: "claude",
      model: "claude:sonnet-5",
      busy: true,
      providerAccountId: "target-account",
      queuedMessages: [queued("a"), row],
      pendingSwitch: {
        from: "codex",
        fromModel: active.model,
        fromSettings: active.modelSettings,
      },
    });
    const snapshot = structuredClone(session);
    expect(queuedMessageForSubmit(session, row.id, "steer", active)).toBe(row);
    const steered = dequeueQueuedMessage(session, row.id);
    expect(steered.queuedMessages?.map((message) => message.id)).toEqual(["a"]);
    expect(steered.harness).toBe("claude");
    expect(steered.providerAccountId).toBe("target-account");
    expect(steered.pendingSwitch).toEqual(session.pendingSwitch);
    expect(session).toEqual(snapshot);
  });

  it("compares effective settings defaults and aliases instead of record identity", () => {
    setHarnessModels("codex", [
      {
        id: active.model,
        harness: "codex",
        name: "Active model",
        settings: [
          {
            id: "reasoningEffort",
            label: "Effort",
            kind: "select",
            value: "high",
            options: [
              { value: "high", label: "High" },
              { value: "xhigh", label: "Extra high" },
            ],
          },
        ],
      },
    ]);
    const row = {
      ...queued("a"),
      selection: {
        ...active,
        modelSettings: { reasoningEffort: "extra-high" },
      },
    };
    const session = chat({ ...active, busy: true, queuedMessages: [row] });
    expect(
      queuedMessageForSubmit(session, row.id, "steer", {
        ...active,
        modelSettings: { reasoningEffort: "xhigh" },
      }),
    ).toBe(row);
    const defaultRow = { ...row, selection: { ...active, modelSettings: {} } };
    expect(
      queuedMessageForSubmit(
        { ...session, queuedMessages: [defaultRow] },
        row.id,
        "steer",
        { ...active, modelSettings: { reasoningEffort: "high" } },
      ),
    ).toBe(defaultRow);
  });

  it("only auto-dispatches the idle head", () => {
    expect(queuedMessageForSubmit(chat(), "a", "dispatch")?.id).toBe("a");
    expect(queuedMessageForSubmit(chat(), "b", "dispatch")).toBeUndefined();
    expect(
      queuedMessageForSubmit(chat({ busy: true }), "a", "dispatch"),
    ).toBeUndefined();
  });

  it("lets Steer target any remaining row, including while busy or paused", () => {
    expect(queuedMessageForSubmit(chat({ busy: true }), "b", "steer")?.id).toBe(
      "b",
    );
    expect(
      queuedMessageForSubmit(chat({ queueStatus: "paused" }), "a", "steer")?.id,
    ).toBe("a");
    expect(queuedMessageForSubmit(chat(), "missing", "steer")).toBeUndefined();
  });
});
