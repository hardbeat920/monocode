import type { HarnessEvent } from "./types";

/**
 * A usage object that adds up every model call in the turn.
 *
 * That sum is spend, not the context window. Grok's `turn_completed.usage`
 * is this shape: `totalTokens` equals input + output across `modelCalls`,
 * so one coding turn reports several times the real window.
 */
export function aggregateModelCalls(usage: Record<string, unknown>): boolean {
  const calls = positive(usage.modelCalls) ?? positive(usage.numTurns);
  return calls != null && calls > 1;
}

/**
 * Keep a live context level reported beside an ACP update.
 *
 * Grok puts the current window occupancy on the envelope `_meta.totalTokens`
 * of every stream chunk. A usage report on the update itself already chose
 * the level, so this does not replace it.
 */
export function withLiveContext(
  params: Record<string, unknown> | null,
  events: HarnessEvent[],
): HarnessEvent[] {
  const used = positive(asRecord(params?._meta)?.totalTokens);
  if (used == null) return events;
  if (events.some((event) => event.type === "context" && event.used != null)) {
    return events;
  }
  return [...events, { type: "context", used }];
}

function positive(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value > 0
    ? value
    : undefined;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}
