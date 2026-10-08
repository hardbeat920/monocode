// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import type { MonoGoal } from "./mono";
import {
  canContinueMonoGoal,
  consumeMonoGoalCommand,
  goalTurnResult,
  isCurrentGoalTurn,
  MONO_GOAL_MAX_TURNS,
} from "./monoGoal";

afterEach(() => {
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
  expect(consumeMonoGoalCommand("/goal Fix the build\nand run checks")).toEqual({
    matched: true,
    action: "start",
    objective: "Fix the build\nand run checks",
  });
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
  expect(canContinueMonoGoal(goal, "goal-1", 4, 4, "completed", false)).toBe(true);
  expect(canContinueMonoGoal(goal, "goal-1", 4, 4, "failed", false)).toBe(false);
  expect(canContinueMonoGoal(goal, "goal-1", 4, 4, "completed", true)).toBe(false);
  expect(canContinueMonoGoal({ ...goal, status: "paused" }, "goal-1", 4, 4, "completed", false)).toBe(false);
});

it("ignores stale completions and stops repeated or unbounded continuation", () => {
  expect(isCurrentGoalTurn({ ...goal, id: "new-goal" }, "goal-1", 4, 4)).toBe(false);
  expect(isCurrentGoalTurn(goal, "goal-1", 4, 5)).toBe(false);
  const repeated = { ...goal, lastReply: "Still checking", stalled: 2 };
  expect(goalTurnResult(repeated, "Still checking")?.status).toBe("blocked");
  expect(goalTurnResult({ ...goal, turns: MONO_GOAL_MAX_TURNS - 1 }, "progress")?.status).toBe("blocked");
});
