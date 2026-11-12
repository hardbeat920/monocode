import { describe, expect, it } from "vitest";
import {
  contextCompactionWindow,
  contextMeterRatio,
  contextPercent,
  contextRatio,
  contextTooltip,
  dropContextWindow,
  formatTokens,
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
});

describe("compaction window", () => {
  const usage = { used: 234_000, window: 1_000_000, compactionWindow: 400_000 };

  it("keeps the capacity ratio and measures the meter against the window", () => {
    expect(contextRatio(usage)).toBe(0.234);
    expect(contextPercent(usage)).toBe(23);
    expect(contextMeterRatio(usage)).toBe(0.585);
  });

  it("reads the compaction window in the tooltip", () => {
    expect(contextTooltip(usage)).toEqual({
      headline: "59% of auto-compaction window",
      detail: "234K / 400K tokens",
      capacity: "Full model capacity: 1M tokens",
    });
  });

  it("handles a smaller model", () => {
    const haiku = { used: 50_000, window: 100_000, compactionWindow: 100_000 };
    expect(contextMeterRatio(haiku)).toBe(0.5);
    expect(contextTooltip(haiku).headline).toBe("50% of auto-compaction window");
  });

  it("leaves readings without a compaction window untouched", () => {
    const plain = { used: 176_000, window: 256_000 };
    expect(contextMeterRatio(plain)).toBe(0.6875);
    expect(contextTooltip(plain)).toStrictEqual({
      headline: "69% context used",
      detail: "176K / 256K tokens",
    });
    expect(contextMeterRatio({ used: 5 })).toBeNull();
    expect(contextMeterRatio(undefined)).toBeNull();
  });

  it("still measures against the window when the full capacity is unknown", () => {
    const partial = { used: 200_000, compactionWindow: 400_000 };
    expect(contextMeterRatio(partial)).toBe(0.5);
    expect(contextTooltip(partial)).toStrictEqual({
      headline: "50% of auto-compaction window",
      detail: "200K / 400K tokens",
    });
  });

  it.each([Number.NaN, Number.POSITIVE_INFINITY, 0, -400_000])(
    "falls back to full capacity for an invalid window (%s)",
    (compactionWindow) => {
      const bad = { used: 234_000, window: 1_000_000, compactionWindow };
      expect(contextCompactionWindow(bad)).toBeUndefined();
      expect(contextMeterRatio(bad)).toBe(0.234);
      expect(contextTooltip(bad)).toStrictEqual({
        headline: "23% context used",
        detail: "234K / 1M tokens",
      });
    },
  );

  it("clamps a window larger than full capacity for display", () => {
    const wide = { used: 500_000, window: 1_000_000, compactionWindow: 2_000_000 };
    expect(contextCompactionWindow(wide)).toBe(1_000_000);
    expect(contextMeterRatio(wide)).toBe(0.5);
    expect(contextTooltip(wide).detail).toBe("500K / 1M tokens");
  });

  it("caps the ring at full when the level overruns the window", () => {
    expect(
      contextMeterRatio({ used: 450_000, window: 1_000_000, compactionWindow: 400_000 }),
    ).toBe(1);
  });

  it("names the window without a percentage when the level is unusable", () => {
    expect(
      contextTooltip({ used: -1, window: 1_000_000, compactionWindow: 400_000 })
        .headline,
    ).toBe("Auto-compaction window");
  });

  it("survives used and window updates, clears on null or invalid", () => {
    const seeded = mergeContextUsage(undefined, {
      used: 10_000,
      window: 1_000_000,
      compactionWindow: 400_000,
    });
    expect(seeded).toEqual({
      used: 10_000,
      window: 1_000_000,
      compactionWindow: 400_000,
    });
    expect(mergeContextUsage(seeded, { used: 20_000 })).toEqual({
      used: 20_000,
      window: 1_000_000,
      compactionWindow: 400_000,
    });
    expect(mergeContextUsage(seeded, { window: 1_200_000 })).toEqual({
      used: 10_000,
      window: 1_200_000,
      compactionWindow: 400_000,
    });
    expect(mergeContextUsage(seeded, { compactionWindow: null })).toEqual({
      used: 10_000,
      window: 1_000_000,
    });
    expect(mergeContextUsage(seeded, { compactionWindow: Number.NaN })).toEqual({
      used: 10_000,
      window: 1_000_000,
    });
    expect(mergeContextUsage(seeded, { compactionWindow: 0 })).toEqual({
      used: 10_000,
      window: 1_000_000,
    });
  });

  it("does not clamp the stored window when capacity is smaller", () => {
    const merged = mergeContextUsage(
      { used: 1, window: 100_000 },
      { compactionWindow: 400_000 },
    );
    expect(merged.compactionWindow).toBe(400_000);
  });

  it("drops with the model-specific window", () => {
    expect(dropContextWindow(usage)).toEqual({ used: 234_000 });
  });
});
