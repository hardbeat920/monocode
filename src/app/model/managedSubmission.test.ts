import { expect, it, vi } from "vitest";
import type { ControlOutcome } from "../../features/orchestration/model/orchestration";
import { ProjectNotFoundError } from "../../features/projects/model/projectLocationError";
import { submitWithSettlement } from "./managedSubmission";
import { submitAfterProjectSync } from "./submissionAcceptance";

function automation() {
  const reservations = new Set(["session"]);
  const updates: ControlOutcome[] = [];
  const onSettled = vi.fn((outcome: ControlOutcome) => {
    updates.push(outcome);
    reservations.delete("session");
  });
  return { reservations, updates, onSettled };
}

it("settles a deferred false result and releases the automation reservation", async () => {
  const { reservations, updates, onSettled } = automation();
  let resolveAcceptance!: (accepted: boolean) => void;
  const deferred = new Promise<boolean>((resolve) => {
    resolveAcceptance = resolve;
  });
  const accepted = submitWithSettlement({
    submit: () => deferred,
    onSettled,
    rejectionMessage: "Run could not start",
  });
  expect(reservations.has("session")).toBe(true);
  expect(updates).toEqual([]);
  resolveAcceptance(false);
  await expect(accepted).resolves.toBe(false);
  expect(reservations.size).toBe(0);
  expect(updates).toEqual([
    { status: "failed", text: "", error: "Run could not start" },
  ]);
});

it("calls the orchestration completion exactly once for a deferred false and a late callback", async () => {
  let settle!: (outcome: ControlOutcome) => void;
  const done = vi.fn();
  await expect(
    submitWithSettlement({
      submit: (onSettled) => {
        settle = onSettled;
        return Promise.resolve(false);
      },
      onSettled: done,
      rejectionMessage: "Turn could not start",
    }),
  ).resolves.toBe(false);
  expect(done).toHaveBeenCalledWith({
    status: "failed",
    text: "",
    error: "Turn could not start",
  });
  settle({ status: "failed", text: "", error: "late failure" });
  settle({ status: "completed", text: "late response" });
  expect(done).toHaveBeenCalledOnce();
});

it.each(["sync failure", "missing project"])(
  "does not double-settle when %s reports through both onSettled and acceptance",
  async (failure) => {
    const { reservations, onSettled } = automation();
    const submit = vi.fn(() => true);
    const accepted = submitWithSettlement({
      submit: (settle) =>
        submitAfterProjectSync({
          cwd: "/repo",
          sync:
            failure === "missing project"
              ? Promise.resolve(null)
              : Promise.reject(new Error("disk unavailable")),
          applyLocationChange: vi.fn(),
          submit,
          onError: (error) =>
            settle({
              status: "failed",
              text: "",
              error: (error as Error).message,
            }),
        }),
      onSettled,
      rejectionMessage: "Run could not start",
    });
    await expect(accepted).resolves.toBe(false);
    expect(submit).not.toHaveBeenCalled();
    expect(reservations.size).toBe(0);
    expect(onSettled).toHaveBeenCalledOnce();
    expect(onSettled.mock.calls[0][0].error).toBe(
      failure === "missing project"
        ? new ProjectNotFoundError("/repo").message
        : "disk unavailable",
    );
  },
);

it.each(["throw", "reject"])(
  "settles a submission that fails with %s before calling onSettled",
  async (failure) => {
    const done = vi.fn();
    const accepted = submitWithSettlement({
      submit: () => {
        if (failure === "throw") throw new Error("submission failed");
        return Promise.reject(new Error("submission failed"));
      },
      onSettled: done,
      rejectionMessage: "Turn could not start",
    });
    await expect(accepted).resolves.toBe(false);
    expect(done).toHaveBeenCalledExactlyOnceWith({
      status: "failed",
      text: "",
      error: "submission failed",
    });
  },
);

it("returns accepted without waiting for the agent, then settles only once when it finishes", async () => {
  const { reservations, onSettled } = automation();
  let settle!: (outcome: ControlOutcome) => void;
  await expect(
    submitWithSettlement({
      submit: (done) => {
        settle = done;
        return Promise.resolve(true);
      },
      onSettled,
      rejectionMessage: "Run could not start",
    }),
  ).resolves.toBe(true);
  expect(onSettled).not.toHaveBeenCalled();
  expect(reservations.has("session")).toBe(true);
  settle({ status: "completed", text: "done" });
  settle({ status: "cancelled", text: "" });
  expect(reservations.size).toBe(0);
  expect(onSettled).toHaveBeenCalledExactlyOnceWith({
    status: "completed",
    text: "done",
  });
});
