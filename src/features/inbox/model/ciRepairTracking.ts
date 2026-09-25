import type { CiRepairRequest } from "./ciRepair";
import { sameProjectPath } from "../../projects/model/recents";

export type CiRepairOutcome = "completed" | "failed" | "cancelled";
export type TrackedCiRepair = CiRepairRequest["target"] & {
  id: string;
  cwd: string;
  sessionId: string;
  startedAt: number;
  sequence?: number;
  phase: "running" | CiRepairOutcome | "interrupted";
};

const KEY = "monocode.ciRepairs.v1";
const ENTRY_PREFIX = `${KEY}.`;
const listeners = new Set<() => void>();
let repairs: readonly TrackedCiRepair[] | undefined;
const interrupted = new Set<string>();
const pendingWrites = new Map<string, TrackedCiRepair | null>();

function newestFirst(a: TrackedCiRepair, b: TrackedCiRepair): number {
  return b.startedAt - a.startedAt || (b.sequence ?? 0) - (a.sequence ?? 0);
}

function loadStored(): TrackedCiRepair[] {
  try {
    let value: unknown[] = [];
    try {
      const legacy: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
      if (Array.isArray(legacy)) value = legacy;
    } catch {
      // New records remain readable if the legacy history is damaged.
    }
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (!key?.startsWith(ENTRY_PREFIX)) continue;
      try {
        value.push(JSON.parse(localStorage.getItem(key) ?? "null"));
      } catch {
        // One damaged record must not hide other repairs.
      }
    }
    return value
      .filter((value): value is TrackedCiRepair => {
        if (!value || typeof value !== "object") return false;
        const item = value as Record<string, unknown>;
        return (
          [item.id, item.cwd, item.sessionId, item.repo, item.headOid].every(
            (s) => typeof s === "string" && s.length > 0,
          ) &&
          typeof item.number === "number" &&
          Number.isSafeInteger(item.number) &&
          item.number > 0 &&
          Number.isFinite(item.startedAt) &&
          (item.sequence === undefined ||
            (typeof item.sequence === "number" &&
              Number.isSafeInteger(item.sequence) &&
              item.sequence >= 0)) &&
          typeof item.phase === "string" &&
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
      .sort(newestFirst);
  } catch {
    return [];
  }
}

function load(): TrackedCiRepair[] {
  const items = new Map(loadStored().map((item) => [item.id, item]));
  for (const [id, repair] of pendingWrites) {
    if (repair) items.set(id, repair);
    else items.delete(id);
  }
  return [...items.values()].sort(newestFirst);
}

function save(repair: TrackedCiRepair, remove = false) {
  const previous = getCiRepairs();
  try {
    const key = `${ENTRY_PREFIX}${repair.id}`;
    if (remove) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(repair));
    pendingWrites.delete(repair.id);
  } catch {
    // Keep tracking in memory if storage is full or unavailable.
    pendingWrites.set(repair.id, remove ? null : repair);
  }
  const combined = new Map(
    [repair, ...previous, ...load()].map((item) => [item.id, item]),
  );
  if (remove) combined.delete(repair.id);
  else combined.set(repair.id, repair);
  const ordered = [...combined.values()].sort(newestFirst);
  for (const item of ordered.slice(200)) {
    pendingWrites.delete(item.id);
    try {
      localStorage.removeItem(`${ENTRY_PREFIX}${item.id}`);
    } catch {
      // The visible history stays bounded when storage is unavailable.
    }
  }
  repairs = ordered
    .slice(0, 200)
    .map((item) =>
      item.phase === "running" && interrupted.has(item.id)
        ? { ...item, phase: "interrupted" }
        : item,
    );
  for (const listener of listeners) listener();
}

export function getCiRepairs(): readonly TrackedCiRepair[] {
  if (!repairs) {
    repairs = load()
      .slice(0, 200)
      .map((item) => {
        if (item.phase !== "running") return item;
        interrupted.add(item.id);
        return { ...item, phase: "interrupted" };
      });
  }
  return repairs;
}

export function subscribeCiRepairs(listener: () => void): () => void {
  if (listeners.size === 0) window.addEventListener("storage", onStorage);
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) window.removeEventListener("storage", onStorage);
  };
}

function onStorage(event: StorageEvent) {
  if (
    event.key !== null &&
    event.key !== KEY &&
    !event.key.startsWith(ENTRY_PREFIX)
  )
    return;
  if (event.key?.startsWith(ENTRY_PREFIX))
    interrupted.delete(event.key.slice(ENTRY_PREFIX.length));
  repairs = load()
    .slice(0, 200)
    .map((item) =>
      item.phase === "running" && interrupted.has(item.id)
        ? { ...item, phase: "interrupted" }
        : item,
    );
  for (const listener of listeners) listener();
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
    sequence:
      Math.max(
        0,
        ...getCiRepairs().map((item) => item.sequence ?? 0),
        ...load().map((item) => item.sequence ?? 0),
      ) + 1,
    phase: "running",
  };
  save(repair);
  try {
    const accepted = submit((phase) => {
      const current = getCiRepairs().find((item) => item.id === repair.id) ?? repair;
      save({ ...current, phase });
    });
    if (!accepted)
      throw new Error(
        "Could not start this fix. Choose another chat and try again.",
      );
  } catch (error) {
    save(repair, true);
    throw error;
  }
}

export function rebaseCiRepairs(from: string, to: string): void {
  for (const repair of load()) {
    if (sameProjectPath(repair.cwd, from)) save({ ...repair, cwd: to });
  }
}
