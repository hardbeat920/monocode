import { describe, expect, it } from "vitest";
import {
  formatResetLine,
  formatSessionDuration,
  formatUsageReport,
  isUsageCommand,
  turnTimeMs,
  usageBar,
} from "./usage";

const KIEV = "Europe/Kiev";
const NOW_SEP13_2PM_KIEV = Date.parse("2026-09-13T11:00:00Z");
const RESET_SEP13_7_50PM_KIEV = Date.parse("2026-09-13T16:50:00Z");
const RESET_SEP15_11AM_KIEV = Date.parse("2026-09-15T08:00:00Z");
const RESET_SEP14_2_30AM_KIEV = Date.parse("2026-09-13T23:30:00Z");

describe("usage command", () => {
  it("matches a standalone /usage command", () => {
    expect(isUsageCommand("/usage")).toBe(true);
    expect(isUsageCommand("  /USAGE\n")).toBe(true);
  });

  it("does not consume ordinary prompt text", () => {
    expect(isUsageCommand("/usage now")).toBe(false);
    expect(isUsageCommand("see /usage")).toBe(false);
    expect(isUsageCommand("/usages")).toBe(false);
  });
});

describe("usageBar", () => {
  it("draws one block per 2% and a half block for an odd percent", () => {
    expect(usageBar(3)).toBe(`█▌${" ".repeat(48)} 3% used`);
    expect(usageBar(27)).toBe(`${"█".repeat(13)}▌${" ".repeat(36)} 27% used`);
    expect(usageBar(48)).toBe(`${"█".repeat(24)}${" ".repeat(26)} 48% used`);
  });

  it("keeps the bar 50 cells wide at the edges", () => {
    expect(usageBar(0)).toBe(`${" ".repeat(50)} 0% used`);
    expect(usageBar(99)).toBe(`${"█".repeat(49)}▌ 99% used`);
    expect(usageBar(100)).toBe(`${"█".repeat(50)} 100% used`);
    expect(usageBar(140)).toBe(`${"█".repeat(50)} 100% used`);
    expect(usageBar(Number.NaN)).toBe(`${" ".repeat(50)} 0% used`);
  });

  it("rounds fractional percentages before drawing", () => {
    expect(usageBar(58.2)).toBe(`${"█".repeat(29)}${" ".repeat(21)} 58% used`);
  });
});

describe("formatResetLine", () => {
  const options = { now: NOW_SEP13_2PM_KIEV, timeZone: KIEV };

  it("prints only the clock time for a same-day reset", () => {
    expect(formatResetLine(RESET_SEP13_7_50PM_KIEV, options)).toBe(
      "Resets 7:50pm (Europe/Kiev)",
    );
  });

  it("prints the date and drops :00 for a later day", () => {
    expect(formatResetLine(RESET_SEP15_11AM_KIEV, options)).toBe(
      "Resets Sep 15 at 11am (Europe/Kiev)",
    );
  });

  it("uses the zone's calendar day, not UTC's", () => {
    expect(formatResetLine(RESET_SEP14_2_30AM_KIEV, options)).toBe(
      "Resets Sep 14 at 2:30am (Europe/Kiev)",
    );
  });

  it("says so when the reset time is unknown", () => {
    expect(formatResetLine(null, options)).toBe("Resets at an unknown time");
  });
});

describe("formatSessionDuration", () => {
  it("floors to whole seconds and omits empty leading units", () => {
    expect(formatSessionDuration(0)).toBe("0s");
    expect(formatSessionDuration(999)).toBe("0s");
    expect(formatSessionDuration(123_000)).toBe("2m 3s");
    expect(formatSessionDuration(3_723_000)).toBe("1h 2m 3s");
    expect(formatSessionDuration(-5)).toBe("0s");
  });
});

describe("turnTimeMs", () => {
  it("sums finished turns and counts an in-flight turn to now", () => {
    const blocks = [
      { id: "u1", role: "user" as const, text: "a", durationMs: 100_000 },
      { id: "a1", role: "assistant" as const, text: "b" },
      {
        id: "u2",
        role: "user" as const,
        text: "c",
        startedAt: NOW_SEP13_2PM_KIEV - 23_000,
      },
      { id: "s1", role: "system" as const, text: "d" },
    ];
    expect(turnTimeMs(blocks, NOW_SEP13_2PM_KIEV)).toBe(123_000);
  });

  it("ignores user blocks without timing", () => {
    expect(
      turnTimeMs([{ id: "u", role: "user", text: "x" }], NOW_SEP13_2PM_KIEV),
    ).toBe(0);
  });
});

describe("formatUsageReport", () => {
  const options = { now: NOW_SEP13_2PM_KIEV, timeZone: KIEV };

  it("renders the four sections in CLI layout", () => {
    const report = formatUsageReport(
      {
        session: {
          blocks: [
            {
              id: "u1",
              role: "user",
              text: "hi",
              durationMs: 123_000,
            },
          ],
          usage: {
            costUsd: 0,
            apiMs: 0,
            tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
            lastProcessCostUsd: 0,
            lastProcessApiMs: 0,
          },
        },
        limits: {
          error: null,
          session: {
            usedPercent: 3,
            windowMinutes: 300,
            resetsAt: RESET_SEP13_7_50PM_KIEV,
          },
          weekly: {
            usedPercent: 27,
            windowMinutes: 10_080,
            resetsAt: RESET_SEP15_11AM_KIEV,
          },
          weeklyByModel: [
            {
              label: "Fable",
              window: {
                usedPercent: 48,
                windowMinutes: 10_080,
                resetsAt: RESET_SEP15_11AM_KIEV,
              },
            },
          ],
        },
      },
      options,
    );
    expect(report).toBe(
      [
        "/usage",
        "",
        "Session",
        "Total cost:            $0.0000",
        "Total duration (API):  0s",
        "Total duration (wall): 2m 3s",
        "Usage:                 0 input, 0 output, 0 cache read, 0 cache write",
        "",
        "Current session",
        `█▌${" ".repeat(48)} 3% used`,
        "Resets 7:50pm (Europe/Kiev)",
        "",
        "Current week (all models)",
        `${"█".repeat(13)}▌${" ".repeat(36)} 27% used`,
        "Resets Sep 15 at 11am (Europe/Kiev)",
        "",
        "Current week (Fable)",
        `${"█".repeat(24)}${" ".repeat(26)} 48% used`,
        "Resets Sep 15 at 11am (Europe/Kiev)",
      ].join("\n"),
    );
  });

  it("renders zero values for numbers no turn has reported yet", () => {
    const report = formatUsageReport(
      {
        session: {
          blocks: [{ id: "u1", role: "user", text: "hi", durationMs: 5_000 }],
        },
        limits: { error: null, session: null, weekly: null },
      },
      options,
    );
    expect(report).toBe(
      [
        "/usage",
        "",
        "Session",
        "Total cost:            $0.0000",
        "Total duration (API):  0s",
        "Total duration (wall): 5s",
        "Usage:                 0 input, 0 output, 0 cache read, 0 cache write",
        "",
        "Rate limits unavailable: no data",
      ].join("\n"),
    );
  });

  it("surfaces the fetch error when no window came back", () => {
    const report = formatUsageReport(
      {
        session: { blocks: [] },
        limits: { error: "Claude not signed in", session: null, weekly: null },
      },
      options,
    );
    expect(
      report.endsWith("Rate limits unavailable: Claude not signed in"),
    ).toBe(true);
  });

  it("omits a window section when that window is missing", () => {
    const report = formatUsageReport(
      {
        session: { blocks: [] },
        limits: {
          error: null,
          session: { usedPercent: 6, windowMinutes: 300, resetsAt: null },
          weekly: null,
          weeklyByModel: [],
        },
      },
      options,
    );
    expect(report).toContain("Current session");
    expect(report).not.toContain("Current week");
    expect(report).not.toContain("Rate limits unavailable");
  });

  it("groups large token counts", () => {
    const report = formatUsageReport(
      {
        session: {
          blocks: [],
          usage: {
            costUsd: 1.5,
            apiMs: 0,
            tokens: {
              input: 1234567,
              output: 89,
              cacheRead: 0,
              cacheWrite: 1000,
            },
            lastProcessCostUsd: 1.5,
            lastProcessApiMs: 0,
          },
        },
        limits: { error: null, session: null, weekly: null },
      },
      options,
    );
    expect(report).toContain(
      "Usage:                 1,234,567 input, 89 output, 0 cache read, 1,000 cache write",
    );
  });
});
