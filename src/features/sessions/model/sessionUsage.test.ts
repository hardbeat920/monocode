import { describe, expect, it } from "vitest";
import {
  formatApiTime,
  formatCost,
  mergeSessionUsage,
  resetProcessCounters,
  sessionUsageTooltip,
} from "./sessionUsage";

describe("mergeSessionUsage", () => {
  it("takes cumulative cost and API time as deltas", () => {
    let usage = mergeSessionUsage(undefined, {
      processCostUsd: 0.01,
      processApiMs: 1_700,
    });
    usage = mergeSessionUsage(usage, {
      processCostUsd: 0.013,
      processApiMs: 2_750,
    });
    expect(usage).toEqual({
      costUsd: 0.013,
      apiMs: 2_750,
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

  it("counts a lower reading as a new process and never lowers the total", () => {
    let usage = mergeSessionUsage(undefined, {
      processCostUsd: 0.5,
      processApiMs: 9_000,
    });
    usage = mergeSessionUsage(usage, {
      processCostUsd: 0.2,
      processApiMs: 1_000,
    });
    expect(usage.costUsd).toBeCloseTo(0.7);
    expect(usage.apiMs).toBe(10_000);
    usage = mergeSessionUsage(usage, { processCostUsd: -1 });
    expect(usage.costUsd).toBeCloseTo(0.7);
  });

  it("leaves untouched fields alone when a result omits them", () => {
    const usage = mergeSessionUsage(
      mergeSessionUsage(undefined, { processCostUsd: 0.3 }),
      { processApiMs: 500 },
    );
    expect(usage.costUsd).toBe(0.3);
    expect(usage.apiMs).toBe(500);
  });
});

describe("formatCost", () => {
  it("rounds to cents", () => {
    expect(formatCost(0.1218)).toBe("$0.12");
    expect(formatCost(12.345)).toBe("$12.35");
    expect(formatCost(0)).toBe("$0.00");
  });

  it("keeps a sub-cent spend visible", () => {
    expect(formatCost(0.004)).toBe("<$0.01");
  });
});

describe("formatApiTime", () => {
  it("scales from seconds to hours", () => {
    expect(formatApiTime(9_600)).toBe("10s");
    expect(formatApiTime(125_000)).toBe("2m 5s");
    expect(formatApiTime(3_780_000)).toBe("1h 3m");
  });
});

describe("sessionUsageTooltip", () => {
  it("heads with the total cost and lists API time", () => {
    expect(
      sessionUsageTooltip({
        costUsd: 0.1218,
        apiMs: 10_400,
        lastProcessCostUsd: 0.1218,
        lastProcessApiMs: 10_400,
      }),
    ).toEqual({
      headline: "Total cost: $0.12",
      details: ["API time: 10s"],
    });
  });

  it("omits API time the harness never reported", () => {
    const tooltip = sessionUsageTooltip({
      costUsd: 0,
      apiMs: 0,
      lastProcessCostUsd: 0,
      lastProcessApiMs: 0,
    });
    expect(tooltip.details).toEqual([]);
  });
});
