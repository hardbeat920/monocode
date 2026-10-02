import { describe, expect, it } from "vitest";
import {
  contextPercent,
  contextRatio,
  contextTooltip,
  dropContextWindow,
  formatTokens,
  markCompacted,
  mergeContextUsage,
} from "./contextUsage";

describe("contextRatio", () => {
  it("divides used by the window", () => {
    expect(contextRatio({ used: 128_000, window: 256_000 })).toBe(0.5);
  });

  it("has no ratio without a window", () => {
    expect(contextRatio({ used: 128_000 })).toBeNull();
    expect(contextRatio(undefined)).toBeNull();
  });

  it("clamps a window overrun to full rather than overflowing the ring", () => {
    expect(contextRatio({ used: 300_000, window: 256_000 })).toBe(1);
  });

  it("rejects nonsense readings", () => {
    expect(contextRatio({ used: -5, window: 100 })).toBeNull();
    expect(contextRatio({ used: 10, window: 0 })).toBeNull();
  });

  it("has no ratio while a compaction leaves the level unverified", () => {
    expect(
      contextRatio({ used: 190_000, window: 200_000, compacted: true }),
    ).toBeNull();
  });
});

describe("contextPercent", () => {
  it("rounds to whole percent", () => {
    expect(contextPercent({ used: 176_000, window: 256_000 })).toBe(69);
  });
});

describe("formatTokens", () => {
  it("keeps small counts exact", () => {
    expect(formatTokens(0)).toBe("0");
    expect(formatTokens(980)).toBe("980");
  });

  it("abbreviates thousands and millions", () => {
    expect(formatTokens(1_000)).toBe("1K");
    expect(formatTokens(1_500)).toBe("1.5K");
    expect(formatTokens(176_000)).toBe("176K");
    expect(formatTokens(1_000_000)).toBe("1M");
    expect(formatTokens(1_200_000)).toBe("1.2M");
  });
});

describe("contextTooltip", () => {
  it("reads like the composer hover", () => {
    expect(contextTooltip({ used: 176_000, window: 256_000 })).toEqual({
      headline: "69% context used",
      detail: "176K / 256K tokens",
    });
  });

  it("omits the denominator when the window is unknown", () => {
    expect(contextTooltip({ used: 176_000 })).toEqual({
      headline: "Context used",
      detail: "176K tokens",
    });
  });

  it("reports the compaction instead of a level it cannot vouch for", () => {
    expect(
      contextTooltip({ used: 190_000, window: 200_000, compacted: true }),
    ).toEqual({
      headline: "Context compacted",
      detail: "Rebuilt inside a 200K token window",
    });
    expect(
      contextTooltip({ used: 190_000, compacted: true }),
    ).toEqual({
      headline: "Context compacted",
      detail: "Rebuilt, measuring again on the next turn",
    });
  });
});

describe("mergeContextUsage", () => {
  it("replaces the level instead of accumulating", () => {
    const first = mergeContextUsage(undefined, { used: 30_000 });
    expect(mergeContextUsage(first, { used: 45_000 })).toEqual({
      used: 45_000,
    });
  });

  it("keeps a window reported on an earlier message", () => {
    const seeded = mergeContextUsage(undefined, {
      used: 10_000,
      window: 200_000,
    });
    expect(mergeContextUsage(seeded, { used: 20_000 })).toEqual({
      used: 20_000,
      window: 200_000,
    });
  });

  it("keeps the level when only a window arrives", () => {
    const seeded = mergeContextUsage(undefined, { used: 10_000 });
    expect(mergeContextUsage(seeded, { window: 200_000 })).toEqual({
      used: 10_000,
      window: 200_000,
    });
  });

  it("drops back down after the harness compacts", () => {
    const full = mergeContextUsage(undefined, {
      used: 190_000,
      window: 200_000,
    });
    expect(mergeContextUsage(full, { used: 40_000 })).toEqual({
      used: 40_000,
      window: 200_000,
    });
  });

  it("keeps the stale marker through a window-only reading", () => {
    // Claude sends the window at a compaction boundary, where the level that
    // accompanies it is the one the compaction replaced. Rebuilding the object
    // here would quietly un-mark it and put the old height back on the ring.
    const marked = markCompacted({ used: 190_000, window: 200_000 });
    expect(marked).toEqual({
      used: 190_000,
      window: 200_000,
      compacted: true,
    });
    expect(mergeContextUsage(marked, { window: 200_000 })).toEqual({
      used: 190_000,
      window: 200_000,
      compacted: true,
    });
  });

  it("clears the stale marker on a reading that carries a level", () => {
    const marked = markCompacted({ used: 190_000, window: 200_000 });
    expect(mergeContextUsage(marked, { used: 40_000 })).toEqual({
      used: 40_000,
      window: 200_000,
    });
  });

  it("clears the stale marker even when the new level is zero", () => {
    const marked = markCompacted({ used: 190_000, window: 200_000 });
    expect(mergeContextUsage(marked, { used: 0, window: 200_000 })).toEqual({
      used: 0,
      window: 200_000,
    });
  });
});

describe("markCompacted", () => {
  it("keeps the level the compaction replaced", () => {
    expect(markCompacted({ used: 190_000, window: 200_000 })).toEqual({
      used: 190_000,
      window: 200_000,
      compacted: true,
    });
  });

  it("has nothing to mark without a reading", () => {
    expect(markCompacted(undefined)).toBeUndefined();
  });
});

describe("dropContextWindow", () => {
  it("keeps the level but forgets the model-specific window", () => {
    expect(dropContextWindow({ used: 30_000, window: 1_000_000 })).toEqual({
      used: 30_000,
    });
  });

  it("passes through nothing", () => {
    expect(dropContextWindow(undefined)).toBeUndefined();
  });

  it("leaves the ring hidden until the next turn re-reports", () => {
    expect(contextRatio(dropContextWindow({ used: 30_000, window: 200_000 }))).toBeNull();
  });

  it("carries the stale marker across a model switch", () => {
    // The marker describes the level rather than the model, so a new model does
    // not vouch for the reading the last one left behind.
    expect(
      dropContextWindow({ used: 190_000, window: 200_000, compacted: true }),
    ).toEqual({ used: 190_000, compacted: true });
  });
});
