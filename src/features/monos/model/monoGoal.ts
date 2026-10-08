import type { MonoGoal } from "./mono";

export const MONO_GOAL_MAX_TURNS = 24;
export const MONO_GOAL_MAX_STALLED_TURNS = 3;

export type MonoGoalCommand =
  | { matched: false }
  | { matched: true; action: "start"; objective: string }
  | { matched: true; action: "status" | "pause" | "resume" | "cancel" };

export function consumeMonoGoalCommand(text: string): MonoGoalCommand {
  const match = text.match(/^\s*\/goal(?=\s|$)\s*([\s\S]*)$/i);
  if (!match) return { matched: false };
  const argument = match[1].trim();
  if (!argument) return { matched: true, action: "status" };
  const action = argument.toLowerCase();
  if (["status", "pause", "resume", "cancel"].includes(action))
    return {
      matched: true,
      action: action as "status" | "pause" | "resume" | "cancel",
    };
  return { matched: true, action: "start", objective: argument };
}

export function goalTurnPrompt(goal: MonoGoal): string {
  return `Continue working toward this goal. Review what has already been done, choose the next useful step, and keep going until it is complete or you are genuinely blocked. When finished or blocked, call app mono.goal.update with this goalId: ${goal.id}.`;
}

export function goalTurnResult(goal: MonoGoal, reply: string): MonoGoal {
  const normalized = reply.trim().replace(/\s+/g, " ").slice(0, 4_000);
  const stalled = !normalized
    ? goal.stalled + 1
    : normalized === goal.lastReply
      ? goal.stalled + 1
      : 1;
  const turns = goal.turns + 1;
  if (turns >= MONO_GOAL_MAX_TURNS)
    return {
      ...goal,
      turns,
      stalled,
      ...(normalized ? { lastReply: normalized } : {}),
      status: "blocked",
      reason: "Automatic continuation stopped after 24 turns.",
    };
  if (stalled >= MONO_GOAL_MAX_STALLED_TURNS)
    return {
      ...goal,
      turns,
      stalled,
      ...(normalized ? { lastReply: normalized } : {}),
      status: "blocked",
      reason: "Automatic continuation stopped after three identical replies.",
    };
  return {
    ...goal,
    turns,
    stalled,
    ...(normalized ? { lastReply: normalized } : {}),
  };
}

export function completeMonoGoalTurn(
  goal: MonoGoal,
  reply: string,
  waiting: boolean,
  alreadyCounted = false,
): { goal: MonoGoal; continue: boolean } {
  const updated = alreadyCounted ? goal : goalTurnResult(goal, reply);
  return { goal: updated, continue: !waiting && updated.status === "active" };
}

export function goalContext(goal: MonoGoal): string {
  if (goal.status !== "active") return "";
  return `<mono_goal>\nThe user explicitly started this goal. Its exact objective is the following JSON string: ${JSON.stringify(goal.objective)}\nKeep working on it across turns. Do not claim it is complete unless it is; call app mono.goal.update with {"goalId":"${goal.id}","status":"done"} when finished, or status "blocked" with a short reason when you cannot proceed. The user can pause or cancel automatic continuation at any time.\n</mono_goal>`;
}

export function isCurrentGoalTurn(
  goal: MonoGoal | undefined,
  goalId: string,
  turn: number,
  currentTurn: number | undefined,
): boolean {
  return (
    goal?.id === goalId && goal.status === "active" && currentTurn === turn
  );
}

export function canContinueMonoGoal(
  goal: MonoGoal | undefined,
  goalId: string,
  turn: number,
  currentTurn: number | undefined,
  outcome: "completed" | "failed" | "cancelled",
  waiting: boolean,
): boolean {
  return (
    isCurrentGoalTurn(goal, goalId, turn, currentTurn) &&
    outcome === "completed" &&
    !waiting
  );
}
