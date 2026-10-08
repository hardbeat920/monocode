// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { submitAfterProjectSync } from "../../../app/model/submissionAcceptance";
import type { MonoGoal } from "./mono";
import {
  canContinueMonoGoal,
  consumeMonoGoalCommand,
  failedMonoGoalSubmission,
  goalTurnResult,
  isActiveMonoGoal,
  isCurrentGoalTurn,
  MONO_GOAL_MAX_TURNS,
  recordMonoGoalTurn,
  submitMonoGoal,
} from "./monoGoal";

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.resetModules();
});

const goal: MonoGoal = {
  id: "goal-1",
  objective: "Fix the failing build",
  status: "active",
  turns: 0,
  stalled: 0,
};

it("parses only leading /goal commands and preserves objective text", () => {
  expect(consumeMonoGoalCommand("/goal Fix the build")).toEqual({
    matched: true,
    action: "start",
    objective: "Fix the build",
  });
  expect(consumeMonoGoalCommand("/goal Fix the build\nand run checks")).toEqual(
    {
      matched: true,
      action: "start",
      objective: "Fix the build\nand run checks",
    },
  );
  expect(consumeMonoGoalCommand(" /goal pause ")).toEqual({
    matched: true,
    action: "pause",
  });
  expect(consumeMonoGoalCommand("Please /goal this later")).toEqual({
    matched: false,
  });
  for (const action of ["status", "pause", "resume", "cancel"] as const)
    expect(consumeMonoGoalCommand(`/goal ${action}`)).toEqual({
      matched: true,
      action,
    });
});

it("persists the visible goal across a Mono roster reload", async () => {
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
    removeItem: (key: string) => stored.delete(key),
  });
  const mono = await import("./mono");
  const created = mono.createMono();
  mono.saveMonoGoal(created.id, goal);
  vi.resetModules();
  const reloaded = await import("./mono");
  expect(reloaded.findMono(created.id)?.goal).toEqual(goal);
  expect(reloaded.monoState({ blocks: [], busy: false }, goal)).toMatchObject({
    status: "idle",
    goal: { objective: goal.objective, status: "active" },
  });
});

it("continues only the same active completed turn when no work is waiting", () => {
  expect(canContinueMonoGoal(goal, "goal-1", 4, 4, "completed", false)).toBe(
    true,
  );
  expect(canContinueMonoGoal(goal, "goal-1", 4, 4, "failed", false)).toBe(
    false,
  );
  expect(canContinueMonoGoal(goal, "goal-1", 4, 4, "completed", true)).toBe(
    false,
  );
  expect(
    canContinueMonoGoal(
      { ...goal, status: "paused" },
      "goal-1",
      4,
      4,
      "completed",
      false,
    ),
  ).toBe(false);
});

it("ignores stale completions and stops repeated or unbounded continuation", () => {
  expect(isCurrentGoalTurn({ ...goal, id: "new-goal" }, "goal-1", 4, 4)).toBe(
    false,
  );
  expect(isCurrentGoalTurn(goal, "goal-1", 4, 5)).toBe(false);
  const repeated = { ...goal, lastReply: "Still checking", stalled: 2 };
  expect(goalTurnResult(repeated, "Still checking")?.status).toBe("blocked");
  expect(
    goalTurnResult({ ...goal, turns: MONO_GOAL_MAX_TURNS - 1 }, "progress")
      ?.status,
  ).toBe("blocked");
});

it("counts delegated report turns before a 0 ms queue invalidates the 50 ms callback", () => {
  vi.useFakeTimers();
  const runReportChain = (reply: (turn: number) => string, count: number) => {
    let currentTurn = 0;
    let currentGoal = goal;
    const continuation = vi.fn();
    for (let index = 0; index < count; index++) {
      const turn = ++currentTurn;
      const goalId = currentGoal.id;
      currentGoal = recordMonoGoalTurn(
        currentGoal,
        reply(index),
        (callback, delay) => window.setTimeout(callback, delay),
        () => {
          if (isCurrentGoalTurn(currentGoal, goalId, turn, currentTurn))
            continuation();
        },
      );
      expect(currentGoal.turns).toBe(index + 1);
      window.setTimeout(() => currentTurn++, 0);
      vi.advanceTimersByTime(0);
      expect(currentTurn).toBe(turn + 1);
      vi.advanceTimersByTime(50);
      expect(continuation).not.toHaveBeenCalled();
    }
    return currentGoal;
  };

  expect(
    runReportChain((turn) => `Report ${turn}`, MONO_GOAL_MAX_TURNS),
  ).toMatchObject({
    status: "blocked",
    turns: MONO_GOAL_MAX_TURNS,
  });
  expect(runReportChain(() => "Delegated report repeats", 3)).toMatchObject({
    status: "blocked",
    turns: 3,
    stalled: 3,
  });
});

it.each([
  ["async false", () => Promise.resolve(false), "paused"],
  ["async rejection", () => Promise.reject(new Error("sync failed")), "paused"],
  ["accepted", () => Promise.resolve(true), "active"],
] as const)(
  "settles a project-bound Mono submission after %s",
  async (_case, acceptance, status) => {
    let currentGoal = goal;
    const onError = vi.fn();
    const settled = submitMonoGoal(
      () =>
        submitAfterProjectSync({
          cwd: "/repo",
          sync: Promise.resolve({
            path: "/repo",
            identity: "repo",
            moved: false,
          }),
          applyLocationChange: vi.fn(),
          submit: acceptance,
          onError,
        }),
      () => {
        currentGoal =
          failedMonoGoalSubmission(
            currentGoal,
            goal.id,
            "paused",
            "The continuation could not start.",
          ) ?? currentGoal;
      },
    );

    await expect(settled).resolves.toBe(status === "active");
    expect(currentGoal.status).toBe(status);
    if (_case === "async rejection") expect(onError).toHaveBeenCalledOnce();
  },
);

it("does not let a stale submission failure overwrite a changed goal", async () => {
  let currentGoal = goal;
  let rejectAcceptance!: (error: Error) => void;
  const settled = submitMonoGoal(
    () =>
      submitAfterProjectSync({
        cwd: "/repo",
        sync: Promise.resolve({
          path: "/repo",
          identity: "repo",
          moved: false,
        }),
        applyLocationChange: vi.fn(),
        submit: () =>
          new Promise<boolean>((_resolve, reject) => {
            rejectAcceptance = reject;
          }),
        onError: vi.fn(),
      }),
    () => {
      currentGoal =
        failedMonoGoalSubmission(
          currentGoal,
          goal.id,
          "paused",
          "The continuation could not start.",
        ) ?? currentGoal;
    },
  );
  await Promise.resolve();
  await Promise.resolve();
  currentGoal = { ...goal, status: "cancelled" };
  rejectAcceptance(new Error("late failure"));

  await expect(settled).resolves.toBe(false);
  expect(currentGoal.status).toBe("cancelled");
});

it.each([
  { ...goal, id: "replacement" },
  { ...goal, status: "paused" as const },
])("ignores a submission failure for a replaced or paused goal", (current) => {
  expect(
    failedMonoGoalSubmission(
      current,
      goal.id,
      "paused",
      "The continuation could not start.",
    ),
  ).toBeUndefined();
});

it("does not submit a project-bound goal after it was cancelled during sync", async () => {
  let currentGoal: MonoGoal | undefined = goal;
  let finishSync!: (location: {
    path: string;
    identity: string;
    moved: false;
  }) => void;
  const submit = vi.fn(() => true);
  const settled = submitMonoGoal(
    () =>
      submitAfterProjectSync({
        cwd: "/repo",
        sync: new Promise((resolve) => {
          finishSync = resolve;
        }),
        applyLocationChange: vi.fn(),
        submit: () => {
          if (!isActiveMonoGoal(currentGoal, goal.id)) return false;
          return submit();
        },
        onError: vi.fn(),
      }),
    () => {
      currentGoal =
        failedMonoGoalSubmission(
          currentGoal,
          goal.id,
          "cancelled",
          "The initial turn could not start.",
        ) ?? currentGoal;
    },
  );
  currentGoal = { ...goal, status: "cancelled" };
  finishSync({ path: "/repo", identity: "repo", moved: false });

  await expect(settled).resolves.toBe(false);
  expect(submit).not.toHaveBeenCalled();
});
