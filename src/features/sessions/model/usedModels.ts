import type { HarnessEvent } from "../../../integrations/harness/core/types";
import { HARNESSES, type HarnessId } from "./session";

const STORAGE_KEY = "monocode.usedModels";
const LIMIT = 10;

export type UsedModel = {
  observationId: string;
  harness: HarnessId;
  model: string;
  identitySource: "requested" | "reported";
  usedAt: number;
};

function validString(value: unknown): value is string {
  return (
    typeof value === "string" && value.trim().length > 0 && value.length <= 512
  );
}

function isUsedModel(value: unknown): value is UsedModel {
  if (!value || typeof value !== "object") return false;
  const item = value as Record<string, unknown>;
  return (
    validString(item.observationId) &&
    validString(item.model) &&
    HARNESSES.includes(item.harness as HarnessId) &&
    (item.identitySource === "requested" ||
      item.identitySource === "reported") &&
    typeof item.usedAt === "number" &&
    Number.isFinite(item.usedAt) &&
    item.usedAt >= 0
  );
}

function recentDistinct(items: UsedModel[]): UsedModel[] {
  const pairs = new Set<string>();
  const observations = new Set<string>();
  return [...items]
    .sort((a, b) => b.usedAt - a.usedAt)
    .filter((item) => {
      const pair = JSON.stringify([item.harness, item.model]);
      if (pairs.has(pair) || observations.has(item.observationId)) return false;
      pairs.add(pair);
      observations.add(item.observationId);
      return true;
    })
    .slice(0, LIMIT)
    .map(({ observationId, harness, model, identitySource, usedAt }) => ({
      observationId,
      harness,
      model,
      identitySource,
      usedAt,
    }));
}

export function readUsedModels(): UsedModel[] {
  try {
    const stored: unknown = JSON.parse(
      localStorage.getItem(STORAGE_KEY) ?? "[]",
    );
    return Array.isArray(stored)
      ? recentDistinct(stored.filter(isUsedModel))
      : [];
  } catch (error: unknown) {
    console.debug("[monocode] used models read", error);
    return [];
  }
}

function save(items: UsedModel[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(recentDistinct(items)));
  } catch (error: unknown) {
    console.debug("[monocode] used models save", error);
  }
}

/** Forward-only actual use, never picker selection. Synchronous read/merge/write
 * serializes same-window updates; simultaneous cross-window writes are best-effort.
 * Only the latest observation per pair is retained, not a historical usage ledger. */
export function recordUsedModel(item: UsedModel): void {
  if (!isUsedModel(item)) return;
  const current = readUsedModels();
  if (current.some((entry) => entry.observationId === item.observationId))
    return;
  save([item, ...current]);
}

/** Correct only this retained observation, preserving its first-activity time.
 * Evicted or superseded observations never resurrect or overwrite newer evidence. */
export function reconcileUsedModel(observationId: string, model: string): void {
  if (!validString(model)) return;
  const current = readUsedModels();
  if (!current.some((item) => item.observationId === observationId)) return;
  save(
    current.map((item) =>
      item.observationId === observationId
        ? { ...item, model, identitySource: "reported" }
        : item,
    ),
  );
}

/** Attach only to real session send events, never title/helper probes. */
export function createTurnModelUsage(
  harness: HarnessId,
  requestedModel: string,
) {
  let model = requestedModel;
  let identitySource: UsedModel["identitySource"] = "requested";
  let observation: { observationId: string; usedAt: number } | undefined;
  let recorded = false;
  const recordObservation = () => {
    if (!observation || !validString(model)) return;
    recordUsedModel({ ...observation, harness, model, identitySource });
    recorded = true;
  };
  return (event: HarnessEvent): void => {
    if (event.type === "session.configChanged" && validString(event.model)) {
      model = event.model;
      identitySource = "reported";
      if (observation) {
        if (recorded) reconcileUsedModel(observation.observationId, model);
        else recordObservation();
      }
    }
    if (observation) return;
    if (
      event.type !== "tool.started" &&
      !(
        (event.type === "message.delta" || event.type === "reasoning.delta") &&
        event.text.length > 0
      )
    )
      return;
    observation = { observationId: crypto.randomUUID(), usedAt: Date.now() };
    recordObservation();
  };
}
