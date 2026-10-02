/**
 * How much of the model context window the session is currently occupying.
 *
 * This is a level, not a running total: every harness reports the size of the
 * prompt it just sent, so the newest reading replaces the previous one. That
 * keeps compaction cheap — once the harness compacts, its next report is simply
 * smaller. It does mean nothing reports the level *at* the boundary, though,
 * so the last reading before a compaction describes a conversation that no
 * longer exists. `compacted` marks that window in which we hold a number we
 * cannot vouch for, until a real measurement replaces it.
 */
export type ContextUsage = {
  /** Tokens in the context window as of the last request. */
  used: number;
  /** Context window for the active model, when the harness reports one. */
  window?: number;
  /**
   * The harness compacted and has not measured the window since.
   *
   * `used` is then the level the compaction replaced, so it must not be shown
   * as the current one. Treated exactly like a model switch: the level is kept
   * because the next reading replaces it, but the ring declines to render a
   * number it cannot vouch for.
   */
  compacted?: boolean;
};

/** Fraction of the window in use, or null when no trustworthy level exists. */
export function contextRatio(usage: ContextUsage | undefined): number | null {
  if (!usage || usage.compacted) return null;
  if (!usage.window || usage.window <= 0) return null;
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
  if (usage.compacted) {
    return {
      headline: "Context compacted",
      detail: usage.window
        ? `Rebuilt inside a ${formatTokens(usage.window)} token window`
        : "Rebuilt, measuring again on the next turn",
    };
  }
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
 *
 * A window on its own is not a measurement. Claude sends one at a compaction
 * boundary, where the level it accompanies is the one the compaction replaced,
 * so the stale marker has to survive a window-only merge and clear only on a
 * reading that actually carries a level.
 */
export function mergeContextUsage(
  previous: ContextUsage | undefined,
  next: { used?: number; window?: number },
): ContextUsage {
  const used = next.used ?? previous?.used ?? 0;
  const window = next.window ?? previous?.window;
  const compacted = next.used === undefined ? previous?.compacted : undefined;
  return {
    used,
    ...(window ? { window } : {}),
    ...(compacted ? { compacted } : {}),
  };
}

/**
 * Mark the held level stale at a compaction boundary.
 *
 * The level is kept rather than zeroed: it is the honest "before" figure, and
 * the next reading replaces it. `contextRatio` refuses to render it meanwhile.
 */
export function markCompacted(
  usage: ContextUsage | undefined,
): ContextUsage | undefined {
  if (!usage) return undefined;
  return { ...usage, compacted: true };
}

/**
 * Forget the window while keeping the level.
 *
 * The window is a property of the model, so switching models invalidates it.
 * The level still roughly holds — it describes the transcript, not the model —
 * and the next turn re-reports both. A stale marker rides along: it describes
 * the level rather than the model, so a model switch does not vouch for it.
 */
export function dropContextWindow(
  usage: ContextUsage | undefined,
): ContextUsage | undefined {
  if (!usage) return undefined;
  return usage.compacted
    ? { used: usage.used, compacted: true }
    : { used: usage.used };
}
