import { describe, expect, it, vi } from "vitest";
import { submitWithSettlement } from "../../../app/model/managedSubmission";
import {
  canDispatchQueuedHead,
  canSteerQueuedHead,
} from "../../sessions/model/messageQueue";
import { newSession, type Session } from "../../sessions/model/session";
import {
  enqueueMonoSessionCompletion,
  monoSessionCompletionResult,
  MonoSessionCompletionBatches,
} from "./monoSessionCompletion";

const origin = { monoId: "mono", turn: 1 };
function result(
  id: string,
  status: "completed" | "failed" | "cancelled" = "completed",
) {
  return monoSessionCompletionResult({
    requestId: id,
    sessionId: id,
    project: "/code/project",
    prompt: `Review ${id}`,
    outcome: {
      status,
      text: `Result ${id}`,
      ...(status === "failed" ? { error: "Launch failed" } : {}),
    },
  });
}

describe("Mono completion batches", () => {
  it("waits for the launching turn and all children, even when a fast child finishes before the others launch", () => {
    const ready = vi.fn();
    const batches = new MonoSessionCompletionBatches(ready);
    const a = batches.watch(origin, "a");
    a(result("a"));
    batches.closeInactive(() => true);
    expect(ready).not.toHaveBeenCalled();
    const b = batches.watch(origin, "b");
    const c = batches.watch(origin, "c");
    c(result("c"));
    batches.closeInactive(() => false);
    expect(ready).not.toHaveBeenCalled();
    b(result("b"));
    expect(ready).toHaveBeenCalledOnce();
    const [monoId, message] = ready.mock.calls[0];
    expect(monoId).toBe("mono");
    expect(message.monoSessionCompletion).toMatchObject({
      sessionCount: 3,
      status: "completed",
    });
    const payload = JSON.parse(message.text.split("\n\n")[1]);
    expect(
      payload.sessions.map((entry: { sessionId: string }) => entry.sessionId),
    ).toEqual(["a", "b", "c"]);
    expect(message.text).toContain("one consolidated update");
    for (const id of ["a", "b", "c"])
      expect(message.text).toContain(`Result ${id}`);
    a(result("a"));
    b(result("b"));
    batches.closeInactive(() => false);
    expect(ready).toHaveBeenCalledOnce();
  });

  it("counts failures and cancellations as finished without releasing partial results", async () => {
    const ready = vi.fn();
    const batches = new MonoSessionCompletionBatches(ready);
    const failed = batches.watch(origin, "failed");
    const cancelled = batches.watch(origin, "cancelled");
    const succeeded = batches.watch(origin, "success");
    batches.closeInactive(() => false);
    await submitWithSettlement({
      submit: () => false,
      onSettled: (outcome) =>
        failed(
          monoSessionCompletionResult({
            requestId: "failed",
            sessionId: "failed",
            project: "/code/project",
            prompt: "Review",
            outcome,
          }),
        ),
      rejectionMessage: "Launch failed",
    });
    cancelled(result("cancelled", "cancelled"));
    expect(ready).not.toHaveBeenCalled();
    succeeded(result("success"));
    const message = ready.mock.calls[0][1];
    expect(message.monoSessionCompletion.status).toBe("failed");
    expect(message.text).toContain("Launch failed");
    expect(message.text).toContain('"status":"cancelled"');
    expect(message.text).toContain('"status":"completed"');
  });

  it("keeps later requests and other Monos independent of a slow earlier batch", () => {
    const ready = vi.fn();
    const batches = new MonoSessionCompletionBatches(ready);
    const slow = batches.watch(origin, "slow");
    batches.watch(origin, "also-slow");
    const later = batches.watch({ ...origin, turn: 2 }, "later");
    const other = batches.watch({ monoId: "another", turn: 1 }, "other");
    batches.closeInactive(() => false);
    later(result("later"));
    other(result("other"));
    expect(ready.mock.calls.map(([monoId]) => monoId)).toEqual([
      "mono",
      "another",
    ]);
    slow(result("slow"));
    expect(ready).toHaveBeenCalledTimes(2);
  });

  it("queues one consolidated report behind chat and never steers a busy Mono", () => {
    let mono: Session = {
      ...newSession("codex", "/code/project"),
      busy: true,
      turnReady: true,
      queuedMessages: [
        { id: "chat", text: "Another question", attachments: [] },
      ],
    };
    const batches = new MonoSessionCompletionBatches((_, message) => {
      mono = enqueueMonoSessionCompletion(mono, message);
    });
    const a = batches.watch(origin, "a");
    const b = batches.watch(origin, "b");
    batches.closeInactive(() => false);
    b(result("b"));
    expect(mono.queuedMessages).toHaveLength(1);
    a(result("a"));
    expect(mono.queuedMessages).toHaveLength(2);
    const notification = {
      ...mono,
      queuedMessages: mono.queuedMessages!.slice(1),
    };
    expect(canDispatchQueuedHead(notification)).toBe(false);
    expect(canSteerQueuedHead(notification)).toBe(false);
    expect(canDispatchQueuedHead({ ...notification, busy: false })).toBe(true);
  });

  it("keeps single-session reports compatible", () => {
    const ready = vi.fn();
    const batches = new MonoSessionCompletionBatches(ready);
    const done = batches.watch(origin, "request");
    const snapshot = result("worker");
    done(snapshot);
    expect(ready).not.toHaveBeenCalled();
    batches.closeInactive(() => false);
    expect(ready.mock.calls[0][1]).toMatchObject({
      id: "mono-completion-request",
      monoSessionCompletion: {
        sessionId: "worker",
        title: "Agent session",
        status: "completed",
      },
    });
    expect(ready.mock.calls[0][1].monoSessionCompletion).not.toHaveProperty(
      "sessionCount",
    );
  });

  it("waits for a retry of a rejected launch and ignores its old settlement callback", () => {
    const ready = vi.fn();
    const batches = new MonoSessionCompletionBatches(ready);
    const rejected = batches.watch(origin, "request");
    rejected(result("worker", "failed"));
    const retry = batches.watch(origin, "request");
    batches.closeInactive(() => false);
    rejected(result("worker", "failed"));
    expect(ready).not.toHaveBeenCalled();
    retry(result("worker"));
    expect(ready).toHaveBeenCalledOnce();
    expect(ready.mock.calls[0][1].monoSessionCompletion.status).toBe(
      "completed",
    );
  });

  it("dismisses the calling Mono's pending cancellation without losing other session results", () => {
    const ready = vi.fn();
    const batches = new MonoSessionCompletionBatches(ready);
    const stopped = batches.watch(origin, "stop-request", "stopped-session");
    const other = batches.watch(origin, "other-request", "other-session");
    batches.closeInactive(() => false);
    batches.dismissSession(origin.monoId, "stopped-session");
    stopped(result("stopped-session", "cancelled"));
    expect(ready).not.toHaveBeenCalled();
    other(result("other-session"));
    expect(ready).toHaveBeenCalledOnce();
    expect(ready.mock.calls[0][1].monoSessionCompletion).toEqual({
      sessionId: "other-session",
      title: "Agent session",
      status: "completed",
    });
    expect(ready.mock.calls[0][1].text).not.toContain("stopped-session");
  });

  it("keeps other Monos and future monitored turns on the same session independent", () => {
    const ready = vi.fn();
    const batches = new MonoSessionCompletionBatches(ready);
    const cancelled = batches.watch(origin, "request", "worker");
    const other = batches.watch(
      { monoId: "another", turn: 1 },
      "other",
      "worker",
    );
    batches.closeInactive(() => false);
    batches.dismissSession(origin.monoId, "worker");
    cancelled(result("worker", "cancelled"));
    other(result("worker", "cancelled"));
    expect(ready.mock.calls.map(([monoId]) => monoId)).toEqual(["another"]);

    const followUp = batches.watch(
      { ...origin, turn: 2 },
      "follow-up",
      "worker",
    );
    batches.closeInactive(() => false);
    cancelled(result("worker", "cancelled"));
    expect(ready).toHaveBeenCalledOnce();
    followUp(result("worker"));
    expect(ready.mock.calls.map(([monoId]) => monoId)).toEqual([
      "another",
      "mono",
    ]);
    expect(ready.mock.calls[1][1].monoSessionCompletion.status).toBe(
      "completed",
    );
  });

  it("does not repeat a rejected launch after the successfully launched session is archived", async () => {
    const ready = vi.fn();
    const batches = new MonoSessionCompletionBatches(ready);
    const rejected = batches.watch(
      origin,
      "rejected-request",
      "failed-session",
    );
    const accepted = await submitWithSettlement({
      submit: () => false,
      onSettled: (outcome) =>
        rejected(
          monoSessionCompletionResult({
            requestId: "rejected-request",
            sessionId: "failed-session",
            project: "/code/project",
            prompt: "Review",
            outcome,
          }),
        ),
      rejectionMessage: "Session was not open",
    });
    expect(accepted).toBe(false);
    rejected.discard();
    const launched = batches.watch(origin, "launched-request", "worker");
    batches.closeInactive(() => false);
    batches.dismissSession(origin.monoId, "worker");
    launched(result("worker", "cancelled"));
    expect(ready).not.toHaveBeenCalled();
  });

  it.each([false, true])(
    "does not deliver a rejected submission when the Mono becomes idle before acceptance (throws: %s)",
    async (throws) => {
      const ready = vi.fn();
      const batches = new MonoSessionCompletionBatches(ready);
      const rejected = batches.watch(origin, "request", "worker", {
        awaitAcceptance: true,
      });
      let finish!: (accepted: boolean) => void;
      let fail!: (error: Error) => void;
      const acceptance = new Promise<boolean>((resolve, reject) => {
        finish = resolve;
        fail = reject;
      });
      const submission = submitWithSettlement({
        submit: () => acceptance,
        onSettled: (outcome) =>
          rejected(
            monoSessionCompletionResult({
              requestId: "request",
              sessionId: "worker",
              project: "/code/project",
              prompt: "Review",
              outcome,
            }),
          ),
        rejectionMessage: "Could not start",
      });
      batches.closeInactive(() => false);
      if (throws) fail(new Error("Session was not open"));
      else finish(false);
      expect(await submission).toBe(false);
      // No delivery may start in the gap before the caller handles rejection.
      expect(ready).not.toHaveBeenCalled();
      rejected.discard();
      rejected.accept();
      rejected(result("worker", "failed"));
      batches.closeInactive(() => false);
      expect(ready).not.toHaveBeenCalled();
    },
  );

  it.each(["completed", "failed", "cancelled"] as const)(
    "keeps an early %s event from an accepted submission and releases it after acceptance",
    (status) => {
      const ready = vi.fn();
      const batches = new MonoSessionCompletionBatches(ready);
      const watched = batches.watch(origin, "request", "worker", {
        awaitAcceptance: true,
      });
      batches.closeInactive(() => false);
      watched(result("worker", status));
      expect(ready).not.toHaveBeenCalled();
      watched.accept();
      expect(ready).toHaveBeenCalledOnce();
      expect(ready.mock.calls[0][1].monoSessionCompletion.status).toBe(status);
      watched.accept();
      expect(ready).toHaveBeenCalledOnce();
    },
  );

  it("releases an accepted sibling when an undecided submission is rejected", () => {
    const ready = vi.fn();
    const batches = new MonoSessionCompletionBatches(ready);
    const rejected = batches.watch(origin, "rejected-request", "rejected", {
      awaitAcceptance: true,
    });
    batches.watch(origin, "accepted-request", "accepted")(result("accepted"));
    batches.closeInactive(() => false);
    rejected(result("rejected", "failed"));
    expect(ready).not.toHaveBeenCalled();
    rejected.discard();
    expect(ready).toHaveBeenCalledOnce();
    expect(ready.mock.calls[0][1].monoSessionCompletion.sessionId).toBe(
      "accepted",
    );
    expect(ready.mock.calls[0][1].text).not.toContain('"sessionId":"rejected"');
  });

  it("lets a discarded request retry while ignoring the old callback and discard", () => {
    const ready = vi.fn();
    const batches = new MonoSessionCompletionBatches(ready);
    const rejected = batches.watch(origin, "request", "worker");
    rejected.discard();
    const retry = batches.watch(origin, "request", "worker");
    rejected(result("worker", "failed"));
    rejected.discard();
    batches.closeInactive(() => false);
    expect(ready).not.toHaveBeenCalled();
    retry(result("worker"));
    expect(ready).toHaveBeenCalledOnce();
    expect(ready.mock.calls[0][1].monoSessionCompletion.status).toBe(
      "completed",
    );
  });

  it.each([false, true])(
    "updates a report dismissed during asynchronous delivery (retain another session: %s)",
    async (retainOther) => {
      let resume!: () => void;
      const waiting = new Promise<void>((resolve) => {
        resume = resolve;
      });
      const delivered = vi.fn();
      const batches = new MonoSessionCompletionBatches(
        async (_, _message, current) => {
          await waiting;
          const message = current();
          if (message) delivered(message);
        },
      );
      batches.watch(origin, "request", "worker")(result("worker", "cancelled"));
      if (retainOther) batches.watch(origin, "other", "other")(result("other"));
      batches.closeInactive(() => false);
      batches.dismissSession(origin.monoId, "worker");
      resume();
      await waiting;
      if (retainOther) {
        expect(delivered).toHaveBeenCalledOnce();
        expect(delivered.mock.calls[0][0].monoSessionCompletion).toEqual({
          sessionId: "other",
          title: "Agent session",
          status: "completed",
        });
        expect(delivered.mock.calls[0][0].text).not.toContain(
          '"sessionId":"worker"',
        );
      } else {
        expect(delivered).not.toHaveBeenCalled();
      }
    },
  );

  it("bounds a large group's reports while retaining every session's identity and outcome", () => {
    const ready = vi.fn();
    const batches = new MonoSessionCompletionBatches(ready);
    for (let index = 0; index < 40; index++) {
      const id = `worker-${index}`;
      batches.watch(
        origin,
        id,
      )({
        ...result(id),
        result: "x".repeat(12_000),
        originalPrompt: "y".repeat(12_000),
      });
    }
    batches.closeInactive(() => false);
    const message = ready.mock.calls[0][1];
    const payload = JSON.parse(message.text.split("\n\n")[1]);
    expect(payload.sessions).toHaveLength(40);
    expect(
      payload.sessions.every(
        (entry: { truncated: boolean }) => entry.truncated,
      ),
    ).toBe(true);
    expect(message.text.length).toBeLessThan(60_000);
  });
});
