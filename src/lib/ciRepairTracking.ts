import type { CiRepairRequest } from "./ciRepair";

export type CiRepairOutcome = "completed" | "failed" | "cancelled";
export type TrackedCiRepair = CiRepairRequest["target"] & {
  id: string;
  cwd: string;
  sessionId: string;
  startedAt: number;
  phase: "running" | CiRepairOutcome | "interrupted";
};

const KEY = "monocode.ciRepairs.v1";
const listeners = new Set<() => void>();
let repairs: readonly TrackedCiRepair[] | undefined;

function load(): TrackedCiRepair[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    if (!Array.isArray(value)) return [];
    return value
      .filter((item): item is TrackedCiRepair => {
        if (!item || typeof item !== "object") return false;
        return (
          [item.id, item.cwd, item.sessionId, item.repo, item.headOid].every(
            (s) => typeof s === "string" && s.length > 0,
          ) &&
          Number.isSafeInteger(item.number) &&
          item.number > 0 &&
          Number.isFinite(item.startedAt) &&
          [
            "running",
            "completed",
            "failed",
            "cancelled",
            "interrupted",
          ].includes(item.phase) &&
          Array.isArray(item.checks) &&
          item.checks.length > 0 &&
          item.checks.every((check: unknown) => {
            if (!check || typeof check !== "object") return false;
            const c = check as Record<string, unknown>;
            return (
              typeof c.name === "string" &&
              typeof c.workflow === "string" &&
              (c.url === null || typeof c.url === "string")
            );
          })
        );
      })
      .slice(0, 200)
      .map((item) => ({
        ...item,
        phase: item.phase === "running" ? "interrupted" : item.phase,
      }));
  } catch {
    return [];
  }
}

function save(next: readonly TrackedCiRepair[]) {
  repairs = next;
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    // Keep tracking in memory if storage is full or unavailable.
  }
  for (const listener of listeners) listener();
}

export function getCiRepairs(): readonly TrackedCiRepair[] {
  return (repairs ??= load());
}

export function subscribeCiRepairs(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function trackCiRepair(
  cwd: string,
  request: CiRepairRequest,
  sessionId: string,
  submit: (settle: (outcome: CiRepairOutcome) => void) => boolean,
): void {
  const repair: TrackedCiRepair = {
    ...request.target,
    id: crypto.randomUUID(),
    cwd,
    sessionId,
    startedAt: Date.now(),
    phase: "running",
  };
  save([repair, ...getCiRepairs()].slice(0, 200));
  try {
    const accepted = submit((phase) => {
      save(
        getCiRepairs().map((item) =>
          item.id === repair.id ? { ...item, phase } : item,
        ),
      );
    });
    if (!accepted)
      throw new Error(
        "Could not start this fix. Choose another chat and try again.",
      );
  } catch (error) {
    save(getCiRepairs().filter((item) => item.id !== repair.id));
    throw error;
  }
}
