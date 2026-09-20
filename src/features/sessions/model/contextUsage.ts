/**
 * How much of the model context window the session is currently occupying.
 *
 * This is a level, not a running total: every harness reports the size of the
 * prompt it just sent, so the newest reading replaces the previous one. That
 * keeps compaction free — once the harness compacts, its next report is simply
 * smaller.
 */
export type ContextUsage = {
  /** Tokens in the context window as of the last request. */
  used: number;
  /** Context window for the active model, when the harness reports one. */
  window?: number;
};

/** Fraction of the window in use, or null when the window is unknown. */
export function contextRatio(usage: ContextUsage | undefined): number | null {
  if (!usage || !usage.window || usage.window <= 0) return null;
  if (!Number.isFinite(usage.used) || usage.used < 0) return null;
  return Math.min(1, usage.used / usage.window);
}

/** Whole-percent context used, or null when the window is unknown. */
export function contextPercent(usage: ContextUsage | undefined): number | null {
  const ratio = contextRatio(usage);
  if (ratio === null) return null;
  return Math.round(ratio * 100);
}

/** Compact token count for chrome: 980, 176K, 1.2M. */
export function formatTokens(count: number): string {
  if (!Number.isFinite(count) || count < 0) return "0";
  if (count < 1000) return String(Math.round(count));
  if (count < 1_000_000) {
    const thousands = count / 1000;
    return `${thousands < 10 ? thousands.toFixed(1).replace(/\.0$/, "") : Math.round(thousands)}K`;
  }
  const millions = count / 1_000_000;
  return `${millions < 10 ? millions.toFixed(1).replace(/\.0$/, "") : Math.round(millions)}M`;
}

/** Two-line hover text: "69% context used" over "176K / 256K tokens". */
export function contextTooltip(usage: ContextUsage): {
  headline: string;
  detail: string;
} {
  const percent = contextPercent(usage);
  return {
    headline:
      percent === null ? "Context used" : `${percent}% context used`,
    detail: usage.window
      ? `${formatTokens(usage.used)} / ${formatTokens(usage.window)} tokens`
      : `${formatTokens(usage.used)} tokens`,
  };
}

/**
 * Merge a fresh reading into what we already know.
 *
 * Harnesses split the two halves across different messages — Claude reports the
 * window only on the turn `result`, well after the first usage arrives — so a
 * reading without a window keeps the last known one.
 */
export function mergeContextUsage(
  previous: ContextUsage | undefined,
  next: { used?: number; window?: number },
): ContextUsage {
  const used = next.used ?? previous?.used ?? 0;
  const window = next.window ?? previous?.window;
  return window ? { used, window } : { used };
}

/**
 * Forget the window while keeping the level.
 *
 * The window is a property of the model, so switching models invalidates it.
 * The level still roughly holds — it describes the transcript, not the model —
 * and the next turn re-reports both.
 */
export function dropContextWindow(
  usage: ContextUsage | undefined,
): ContextUsage | undefined {
  if (!usage) return undefined;
  return { used: usage.used };
}
