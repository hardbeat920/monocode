export type TurnUsage = {
  processCostUsd?: number;
  processApiMs?: number;
};

export type SessionUsage = {
  costUsd: number;
  apiMs: number;
  lastProcessCostUsd: number;
  lastProcessApiMs: number;
};

const EMPTY_USAGE: SessionUsage = {
  costUsd: 0,
  apiMs: 0,
  lastProcessCostUsd: 0,
  lastProcessApiMs: 0,
};

export function mergeSessionUsage(
  previous: SessionUsage | undefined,
  turn: TurnUsage,
): SessionUsage {
  const base = previous ?? EMPTY_USAGE;
  const next = { ...base };
  if (turn.processCostUsd != null) {
    next.costUsd += processDelta(turn.processCostUsd, base.lastProcessCostUsd);
    next.lastProcessCostUsd = turn.processCostUsd;
  }
  if (turn.processApiMs != null) {
    next.apiMs += processDelta(turn.processApiMs, base.lastProcessApiMs);
    next.lastProcessApiMs = turn.processApiMs;
  }
  return next;
}

/** A reading below the last one comes from a new process whose counter restarted. */
function processDelta(reading: number, last: number): number {
  return reading < last ? Math.max(0, reading) : reading - last;
}

export function resetProcessCounters(usage: SessionUsage): SessionUsage {
  return { ...usage, lastProcessCostUsd: 0, lastProcessApiMs: 0 };
}

export function formatCost(costUsd: number): string {
  if (costUsd > 0 && costUsd < 0.01) return "<$0.01";
  return `$${costUsd.toFixed(2)}`;
}

export function formatApiTime(ms: number): string {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const minutes = Math.floor(totalSeconds / 60);
  if (minutes < 60) return `${minutes}m ${totalSeconds % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

export function sessionUsageTooltip(usage: SessionUsage): {
  headline: string;
  details: string[];
} {
  return {
    headline: `Total cost: ${formatCost(usage.costUsd)}`,
    details: usage.apiMs > 0 ? [`API time: ${formatApiTime(usage.apiMs)}`] : [],
  };
}
