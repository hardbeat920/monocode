import { describe, expect, it } from "vitest";
import { mergeSessionUsage, resetProcessCounters } from "./sessionUsage";

describe("mergeSessionUsage", () => {
  it("takes cumulative cost and API time as deltas and sums tokens", () => {
    let usage = mergeSessionUsage(undefined, {
      processCostUsd: 0.01,
      processApiMs: 1_700,
      turnTokens: {
        input: 10,
        output: 62,
        cacheRead: 15_118,
        cacheWrite: 4_522,
      },
    });
    usage = mergeSessionUsage(usage, {
      processCostUsd: 0.013,
      processApiMs: 2_750,
      turnTokens: { input: 10, output: 31, cacheRead: 19_640, cacheWrite: 139 },
    });
    expect(usage).toEqual({
      costUsd: 0.013,
      apiMs: 2_750,
      tokens: { input: 20, output: 93, cacheRead: 34_758, cacheWrite: 4_661 },
      lastProcessCostUsd: 0.013,
      lastProcessApiMs: 2_750,
    });
  });

  it("adds a new process's counters on top after a reset", () => {
    let usage = mergeSessionUsage(undefined, {
      processCostUsd: 0.5,
      processApiMs: 9_000,
    });
    usage = mergeSessionUsage(resetProcessCounters(usage), {
      processCostUsd: 0.2,
      processApiMs: 1_000,
    });
    expect(usage.costUsd).toBeCloseTo(0.7);
    expect(usage.apiMs).toBe(10_000);
    expect(usage.lastProcessCostUsd).toBe(0.2);
  });

  it("leaves untouched fields alone when a result omits them", () => {
    const usage = mergeSessionUsage(
      mergeSessionUsage(undefined, { processCostUsd: 0.3 }),
      { turnTokens: { input: 1, output: 2, cacheRead: 3, cacheWrite: 4 } },
    );
    expect(usage.costUsd).toBe(0.3);
    expect(usage.apiMs).toBe(0);
    expect(usage.tokens).toEqual({
      input: 1,
      output: 2,
      cacheRead: 3,
      cacheWrite: 4,
    });
  });
});
