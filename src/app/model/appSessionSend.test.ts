import { describe, expect, it, vi } from "vitest";
import type { ControlOutcome } from "../../features/orchestration/model/orchestration";
import { appendPreparingHandoff } from "../../features/sessions/model/handoff";
import { dequeueQueuedMessage } from "../../features/sessions/model/messageQueue";
import {
  newSession,
  type QueuedMessage,
  type Session,
} from "../../features/sessions/model/session";
import { appendUser } from "../../integrations/harness/core/apply";
import {
  createTurnWatchers,
  deliverAppSteer,
  forgetAppSteer,
  planAppSend,
  priorAppSend,
  QUEUED_FOLLOW_UP_ARCHIVED,
  QUEUED_FOLLOW_UP_REMOVED,
  queueAppFollowUp,
  queuedAppDelivery,
  recordAppSteer,
  SESSION_BUSY_ERROR,
  steerBlockReason,
  steerRunningTurn,
  TURN_FINISHED_ERROR,
  type TurnWatchers,
} from "./appSessionSend";

const prompt = "Also cover the empty input";

function busySession(): Session {
  return appendUser(newSession("codex", "/repo"), "Fix the parser");
}

function plan(input: Partial<Parameters<typeof planAppSend>[0]>) {
  return planAppSend({
    harness: "codex",
    busy: true,
    steerable: true,
    mono: false,
    ...input,
  });
}

describe("planAppSend", () => {
  it.each([false, true])(
    "refuses a busy session when the request has no whenBusy (mono: %s)",
    (mono) => {
      expect(() => plan({ mono })).toThrow(SESSION_BUSY_ERROR);
    },
  );

  it.each([undefined, "steer", "followUp"] as const)(
    "sends to an idle session whatever whenBusy asks (%s)",
    (whenBusy) => {
      expect(
        plan({
          busy: false,
          whenBusy,
          steerable: false,
          steerBlocked: "Preparing worktree",
        }),
      ).toBe("send");
      expect(plan({ busy: false, whenBusy, mono: true })).toBe("send");
    },
  );

  it("queues a follow-up on a busy session, even one that cannot be steered", () => {
    expect(
      plan({
        whenBusy: "followUp",
        steerable: false,
        steerBlocked: "Preparing worktree",
      }),
    ).toBe("queue");
  });

  it("steers a busy session that can take a steer", () => {
    expect(plan({ whenBusy: "steer" })).toBe("steer");
  });

  it("rejects a steer the harness cannot take instead of queueing it", () => {
    expect(() =>
      plan({ harness: "fx", whenBusy: "steer", steerable: false }),
    ).toThrow("fx cannot be steered mid-turn");
  });

  it("rejects a CLI steer to Hermes, which may queue it as a follow-up", () => {
    expect(() => plan({ harness: "hermes", whenBusy: "steer" })).toThrow(
      "hermes cannot be steered mid-turn",
    );
    expect(plan({ harness: "hermes", whenBusy: "followUp" })).toBe("queue");
  });

  it("rejects a steer with the reason the session state blocks it", () => {
    const reason =
      'The session is switching providers and cannot be steered; use whenBusy "followUp"';
    expect(() => plan({ whenBusy: "steer", steerBlocked: reason })).toThrow(
      reason,
    );
  });

  it.each(["steer", "followUp"] as const)(
    "refuses whenBusy %s in a busy Mono's own conversation",
    (whenBusy) => {
      expect(() => plan({ mono: true, whenBusy })).toThrow(
        "A Mono's own conversation does not accept whenBusy",
      );
    },
  );
});

describe("steerBlockReason", () => {
  it("blocks a steer while the session prepares its worktree", () => {
    expect(
      steerBlockReason({ ...busySession(), worktreePreparing: true }, prompt),
    ).toMatch(/preparing its worktree/);
  });

  it("blocks a steer while the session switches providers", () => {
    const switching: Session = {
      ...busySession(),
      pendingSwitch: { from: "claude", fromModel: "sonnet", fromSettings: {} },
    };
    const handingOff = appendPreparingHandoff(busySession(), "claude", "codex");
    expect(steerBlockReason(switching, prompt)).toMatch(/switching providers/);
    expect(steerBlockReason(handingOff, prompt)).toMatch(/switching providers/);
  });

  it("blocks an /operator prompt, which starts its own turn", () => {
    expect(
      steerBlockReason(busySession(), "/operator list my sessions"),
    ).toMatch(/^\/operator starts a new turn/);
  });

  it("lets a plain prompt steer a plain busy session", () => {
    expect(steerBlockReason(busySession(), prompt)).toBeUndefined();
  });
});

describe("deliverAppSteer", () => {
  function monitor() {
    return Object.assign(vi.fn(), { accept: vi.fn(), discard: vi.fn() });
  }

  it("records the steer first and answers only after the harness accepts it", async () => {
    const calls: string[] = [];
    let acknowledge!: () => void;
    const acknowledged = new Promise<void>((resolve) => {
      acknowledge = resolve;
    });
    const watchers = createTurnWatchers();
    const watcher = monitor();
    let settled = false;
    const delivery = deliverAppSteer({
      watchers,
      sessionId: "session-a",
      gen: 1,
      watcher,
      record: () => calls.push("record"),
      steer: () => {
        calls.push("steer");
        return acknowledged;
      },
      forget: () => calls.push("forget"),
    }).finally(() => {
      settled = true;
    });
    await Promise.resolve();
    expect(calls).toEqual(["record", "steer"]);
    expect(settled).toBe(false);
    expect(watcher.accept).not.toHaveBeenCalled();
    acknowledge();
    await expect(delivery).resolves.toEqual({
      alreadySubmitted: false,
      delivery: "steered",
    });
    expect(calls).toEqual(["record", "steer"]);
    expect(watcher.accept).toHaveBeenCalledOnce();
    expect(watcher.discard).not.toHaveBeenCalled();
    watchers.settle("session-a", 1, { status: "completed", text: "Done" });
    expect(watcher).toHaveBeenCalledWith({ status: "completed", text: "Done" });
  });

  it("forgets the recorded steer and rethrows the harness error when the steer fails", async () => {
    const failure = new Error("The turn ended before the steer arrived");
    const forget = vi.fn();
    const watchers = createTurnWatchers();
    const watcher = monitor();
    await expect(
      deliverAppSteer({
        watchers,
        sessionId: "session-a",
        gen: 1,
        watcher,
        record: vi.fn(),
        steer: () => Promise.reject(failure),
        forget,
      }),
    ).rejects.toBe(failure);
    expect(forget).toHaveBeenCalledOnce();
    expect(watcher.discard).toHaveBeenCalledOnce();
    expect(watcher.accept).not.toHaveBeenCalled();
    watchers.settle("session-a", 1, { status: "completed", text: "" });
    expect(watcher).not.toHaveBeenCalled();
  });

  it("refuses a turn that already ended before recording anything, even without a watcher", async () => {
    const watchers = createTurnWatchers();
    watchers.settle("session-a", 1, { status: "completed", text: "" });
    const record = vi.fn();
    const steer = vi.fn(() => Promise.resolve());
    await expect(
      deliverAppSteer({
        watchers,
        sessionId: "session-a",
        gen: 1,
        record,
        steer,
        forget: vi.fn(),
      }),
    ).rejects.toThrow(TURN_FINISHED_ERROR);
    expect(record).not.toHaveBeenCalled();
    expect(steer).not.toHaveBeenCalled();
  });
});

describe("recordAppSteer", () => {
  it("does not mark a session busy, so a rejected steer cannot leave it stuck", () => {
    const idle: Session = { ...busySession(), busy: false };
    const recorded = recordAppSteer(idle, prompt, "request-1");
    expect(recorded.busy).toBe(false);
    expect(forgetAppSteer(recorded, "request-1")).toEqual(idle);
    expect(recordAppSteer(busySession(), prompt, "request-1").busy).toBe(true);
  });
});

describe("steerRunningTurn for a queued CLI follow-up", () => {
  const completed: ControlOutcome = { status: "completed", text: "Turn N" };

  function steerQueued(watchers: TurnWatchers, steer: () => Promise<void>) {
    const steps = {
      record: vi.fn(),
      steer: vi.fn(steer),
      fail: vi.fn(),
    };
    const steered = steerRunningTurn({
      watchers,
      sessionId: "session-a",
      gen: 4,
      queued: { id: "queued-1" },
      fallbackError: "codex could not steer the active turn",
      ...steps,
    });
    return { steered, ...steps };
  }

  it("reports a failed steer to the watcher instead of the outcome of the turn it missed", async () => {
    const watchers = createTurnWatchers();
    const watcher = vi.fn();
    watchers.hold("queued-1", watcher);
    const { steered, fail } = steerQueued(watchers, () =>
      Promise.reject(new Error("No active turn to steer")),
    );
    expect(steered).toBe(true);
    await vi.waitFor(() => expect(fail).toHaveBeenCalled());
    watchers.settle("session-a", 4, completed);
    expect(watcher).toHaveBeenCalledOnce();
    expect(watcher).toHaveBeenCalledWith({
      status: "failed",
      text: "",
      error: "No active turn to steer",
    });
    expect(fail).toHaveBeenCalledWith("No active turn to steer");
  });

  it("does not report twice when the turn settled before the steer failed", async () => {
    const watchers = createTurnWatchers();
    const watcher = vi.fn();
    watchers.hold("queued-1", watcher);
    let reject!: (error: Error) => void;
    const { fail } = steerQueued(
      watchers,
      () =>
        new Promise((_resolve, rejectSteer) => {
          reject = rejectSteer;
        }),
    );
    watchers.settle("session-a", 4, completed);
    reject(new Error("No active turn to steer"));
    await vi.waitFor(() => expect(fail).toHaveBeenCalled());
    expect(watcher).toHaveBeenCalledOnce();
    expect(watcher).toHaveBeenCalledWith(completed);
  });

  it("steers nothing into a turn that already ended and keeps the item's watcher", () => {
    const watchers = createTurnWatchers();
    const watcher = vi.fn();
    watchers.hold("queued-1", watcher);
    watchers.settle("session-a", 4, completed);
    const { steered, record, steer } = steerQueued(watchers, () =>
      Promise.resolve(),
    );
    expect(steered).toBe(false);
    expect(record).not.toHaveBeenCalled();
    expect(steer).not.toHaveBeenCalled();
    expect(watcher).not.toHaveBeenCalled();
    expect(watchers.take("queued-1")).toBe(watcher);
  });

  it("lets the watcher follow the running turn when the steer succeeds", async () => {
    const watchers = createTurnWatchers();
    const watcher = vi.fn();
    watchers.hold("queued-1", watcher);
    const { steered, steer, fail } = steerQueued(watchers, () =>
      Promise.resolve(),
    );
    expect(steered).toBe(true);
    await steer.mock.results[0].value;
    watchers.settle("session-a", 4, completed);
    expect(watcher).toHaveBeenCalledWith(completed);
    expect(fail).not.toHaveBeenCalled();
  });
});

describe("priorAppSend on a retried request", () => {
  it("reports a recorded steer as already steered", () => {
    const steered = recordAppSteer(busySession(), prompt, "request-1");
    expect(priorAppSend(steered, "request-1", prompt)).toEqual({
      alreadySubmitted: true,
      delivery: "steered",
    });
  });

  it("does not report a steer the harness rejected as delivered", () => {
    const session = busySession();
    const forgotten = forgetAppSteer(
      recordAppSteer(session, prompt, "request-1"),
      "request-1",
    );
    expect(forgotten.blocks).toEqual(session.blocks);
    expect(priorAppSend(forgotten, "request-1", prompt)).toBeUndefined();
  });

  it("reports a queued follow-up as queued, even after the user edits it", () => {
    const queued = queueAppFollowUp(busySession(), {
      id: "queued-1",
      prompt,
      requestId: "request-2",
    });
    const edited: Session = {
      ...queued,
      queuedMessages: queued.queuedMessages?.map((message) => ({
        ...message,
        text: "Cover the empty input first",
      })),
    };
    const result = {
      alreadySubmitted: true,
      delivery: "queued",
      queuedMessageId: "queued-1",
    };
    expect(priorAppSend(queued, "request-2", prompt)).toEqual(result);
    expect(priorAppSend(edited, "request-2", prompt)).toEqual(result);
  });

  it("reports a sent queued follow-up as queued, even with its edited text", () => {
    const queued = queueAppFollowUp(busySession(), {
      id: "queued-1",
      prompt,
      requestId: "request-2",
    });
    const message = {
      ...queued.queuedMessages![0],
      text: "Cover the empty input first",
    };
    const sent = appendUser(
      dequeueQueuedMessage(queued, message.id),
      message.text,
      [],
      queuedAppDelivery(message),
    );
    expect(priorAppSend(sent, "request-2", prompt)).toEqual({
      alreadySubmitted: true,
      delivery: "queued",
      queuedMessageId: "queued-1",
    });
  });

  it("reports an earlier plain send as sent", () => {
    const sent = appendUser(newSession("codex", "/repo"), prompt, [], {
      appRequestId: "request-3",
    });
    expect(priorAppSend(sent, "request-3", prompt)).toEqual({
      alreadySubmitted: true,
      delivery: "sent",
    });
  });

  it.each([
    [
      "sent",
      () =>
        appendUser(newSession("codex", "/repo"), prompt, [], {
          appRequestId: "request-3",
        }),
    ],
    ["steered", () => recordAppSteer(busySession(), prompt, "request-3")],
  ])(
    "refuses a %s request ID reused with another prompt",
    (_delivery, earlier) => {
      expect(() =>
        priorAppSend(earlier(), "request-3", "Delete the parser tests"),
      ).toThrow("Request ID was already used with another prompt");
    },
  );

  it("refuses a request ID that belongs to an unsent draft", () => {
    const drafted: Pick<Session, "blocks"> = {
      blocks: [
        {
          id: "draft",
          role: "user",
          text: prompt,
          draft: true,
          appRequestId: "request-4",
        },
      ],
    };
    expect(() => priorAppSend(drafted, "request-4", prompt)).toThrow(
      "Request ID belongs to an unsent draft",
    );
  });
});

describe("queueAppFollowUp", () => {
  it.each([
    [undefined, "active"],
    ["resuming", "active"],
    ["paused", "paused"],
  ] as const)(
    "adds one follow-up behind earlier items (queue %s becomes %s)",
    (queueStatus, expected) => {
      const earlier: QueuedMessage = {
        id: "composer-1",
        text: "First read the logs",
        attachments: [],
      };
      const session = queueAppFollowUp(
        { ...busySession(), queuedMessages: [earlier], queueStatus },
        { id: "queued-1", prompt, requestId: "request-5" },
      );
      expect(session.queuedMessages).toEqual([
        earlier,
        {
          id: "queued-1",
          text: prompt,
          attachments: [],
          appRequestId: "request-5",
        },
      ]);
      expect(session.queueStatus).toBe(expected);
    },
  );
});

describe("createTurnWatchers for notifyOnComplete", () => {
  const completed: ControlOutcome = {
    status: "completed",
    text: "Fixed the parser",
  };
  const failed: ControlOutcome = {
    status: "failed",
    text: "",
    error: "Provider exited",
  };

  it("notifies a watcher once, when its own turn settles", () => {
    const watchers = createTurnWatchers();
    const watcher = vi.fn();
    expect(watchers.attach("session-a", 2, watcher)).toBe(true);
    watchers.settle("session-a", 1, failed);
    watchers.settle("session-b", 2, failed);
    expect(watcher).not.toHaveBeenCalled();
    watchers.settle("session-a", 2, completed);
    watchers.settle("session-a", 2, failed);
    expect(watcher).toHaveBeenCalledOnce();
    expect(watcher).toHaveBeenCalledWith(completed);
  });

  it("still notifies an older turn that settles after a newer one", () => {
    const watchers = createTurnWatchers();
    const watcher = vi.fn();
    watchers.attach("session-a", 1, watcher);
    watchers.settle("session-a", 2, completed);
    watchers.settle("session-a", 1, failed);
    expect(watcher).toHaveBeenCalledOnce();
    expect(watcher).toHaveBeenCalledWith(failed);
  });

  it("refuses to attach to a settled or older turn of the same session", () => {
    const watchers = createTurnWatchers();
    const late = vi.fn();
    watchers.settle("session-a", 3, completed);
    expect(watchers.attach("session-a", 3, late)).toBe(false);
    expect(watchers.attach("session-a", 2, late)).toBe(false);
    watchers.settle("session-a", 3, completed);
    watchers.settle("session-a", 2, completed);
    expect(late).not.toHaveBeenCalled();
    expect(watchers.attach("session-a", 4, vi.fn())).toBe(true);
    expect(watchers.attach("session-b", 3, vi.fn())).toBe(true);
  });

  it("hands a held follow-up watcher over only once", () => {
    const watchers = createTurnWatchers();
    const watcher = vi.fn();
    watchers.hold("queued-1", watcher);
    expect(watchers.take("queued-1")).toBe(watcher);
    expect(watchers.take("queued-1")).toBeUndefined();
    expect(watcher).not.toHaveBeenCalled();
  });

  it("notifies a queued follow-up steered into the running turn when that turn settles", () => {
    const watchers = createTurnWatchers();
    const watcher = vi.fn();
    watchers.hold("queued-1", watcher);
    const moved = watchers.take("queued-1");
    expect(moved).toBe(watcher);
    expect(watchers.attach("session-a", 5, moved!)).toBe(true);
    watchers.cancel("queued-1");
    watchers.settle("session-a", 4, failed);
    expect(watcher).not.toHaveBeenCalled();
    watchers.settle("session-a", 5, completed);
    expect(watcher).toHaveBeenCalledOnce();
    expect(watcher).toHaveBeenCalledWith(completed);
  });

  it("tells the CLI when the user removes a queued follow-up before it is sent", () => {
    const watchers = createTurnWatchers();
    const watcher = vi.fn();
    watchers.hold("queued-1", watcher);
    watchers.cancel("queued-1");
    watchers.cancel("queued-1");
    expect(watcher).toHaveBeenCalledOnce();
    expect(watcher).toHaveBeenCalledWith({
      status: "cancelled",
      text: "",
      error: QUEUED_FOLLOW_UP_REMOVED,
    });
    expect(watchers.take("queued-1")).toBeUndefined();
  });

  it("tells the CLI when archiving the session leaves its follow-up unsent", () => {
    const watchers = createTurnWatchers();
    const watcher = vi.fn();
    watchers.hold("queued-1", watcher);
    watchers.cancel("queued-1", QUEUED_FOLLOW_UP_ARCHIVED);
    expect(watcher).toHaveBeenCalledWith({
      status: "cancelled",
      text: "",
      error: QUEUED_FOLLOW_UP_ARCHIVED,
    });
  });

  it("does not notify a watcher detached from its turn", () => {
    const watchers = createTurnWatchers();
    const detached = vi.fn();
    const attached = vi.fn();
    watchers.attach("session-a", 1, detached);
    watchers.attach("session-a", 1, attached);
    watchers.detach("session-a", 1, detached);
    watchers.settle("session-a", 1, completed);
    expect(detached).not.toHaveBeenCalled();
    expect(attached).toHaveBeenCalledWith(completed);
  });

  it("reports a turn as watched only until it settles or loses its last watcher", () => {
    const watchers = createTurnWatchers();
    const first = vi.fn();
    const second = vi.fn();
    expect(watchers.watching("session-a", 1)).toBe(false);
    watchers.attach("session-a", 1, first);
    watchers.attach("session-a", 1, second);
    expect(watchers.watching("session-a", 1)).toBe(true);
    expect(watchers.watching("session-a", 2)).toBe(false);
    expect(watchers.watching("session-b", 1)).toBe(false);
    watchers.detach("session-a", 1, first);
    expect(watchers.watching("session-a", 1)).toBe(true);
    watchers.detach("session-a", 1, second);
    expect(watchers.watching("session-a", 1)).toBe(false);
    watchers.attach("session-a", 2, first);
    expect(watchers.watching("session-a", 2)).toBe(true);
    watchers.settle("session-a", 2, completed);
    expect(watchers.watching("session-a", 2)).toBe(false);
  });
});
