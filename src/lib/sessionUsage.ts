export type UsageTokens = {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
};

export type TurnUsage = {
  processCostUsd?: number;
  processApiMs?: number;
  turnTokens?: UsageTokens;
};

export type SessionUsage = {
  costUsd: number;
  apiMs: number;
  tokens: UsageTokens;
  lastProcessCostUsd: number;
  lastProcessApiMs: number;
};

const EMPTY_USAGE: SessionUsage = {
  costUsd: 0,
  apiMs: 0,
  tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  lastProcessCostUsd: 0,
  lastProcessApiMs: 0,
};

export function mergeSessionUsage(
  previous: SessionUsage | undefined,
  turn: TurnUsage,
): SessionUsage {
  const base = previous ?? EMPTY_USAGE;
  const next = { ...base, tokens: { ...base.tokens } };
  if (turn.processCostUsd != null) {
    next.costUsd += turn.processCostUsd - base.lastProcessCostUsd;
    next.lastProcessCostUsd = turn.processCostUsd;
  }
  if (turn.processApiMs != null) {
    next.apiMs += turn.processApiMs - base.lastProcessApiMs;
    next.lastProcessApiMs = turn.processApiMs;
  }
  if (turn.turnTokens) {
    next.tokens.input += turn.turnTokens.input;
    next.tokens.output += turn.turnTokens.output;
    next.tokens.cacheRead += turn.turnTokens.cacheRead;
    next.tokens.cacheWrite += turn.turnTokens.cacheWrite;
  }
  return next;
}

export function resetProcessCounters(usage: SessionUsage): SessionUsage {
  return { ...usage, lastProcessCostUsd: 0, lastProcessApiMs: 0 };
}
