import { describe, expect, it } from "vitest";
import {
  clampUsedPercent,
  formatRateLimitWindowChipLabel,
  formatResetCountdown,
  formatResetDuration,
  formatUsagePercent,
  formatWindowLabel,
  idleRateLimits,
  isRateLimitProvider,
  isRateLimitSnapshotStale,
  mapUsageWindow,
  parseClaudeOAuthUsage,
  parseCodexRateLimits,
  parseCursorUsageSummary,
  parseGrokBilling,
  parseResetTimestamp,
  RATE_LIMIT_MIN_REFETCH_MS,
  rateLimitWindowTooltip,
  sharedWindowResetLabel,
  shouldFetchProvider,
  shouldFetchRateLimits,
  usageFooterProviders,
} from "./rateLimits";

describe("formatWindowLabel", () => {
  it("uses the compact 5h / wk labels", () => {
    expect(formatWindowLabel(300)).toBe("5h");
    expect(formatWindowLabel(10_080)).toBe("wk");
    expect(formatWindowLabel(60)).toBe("1h");
    expect(formatWindowLabel(45)).toBe("45m");
    expect(formatWindowLabel(1_440)).toBe("1d");
  });
});

describe("formatResetDuration", () => {
  it("floors to whole units", () => {
    expect(formatResetDuration(47 * 60_000)).toBe("47m");
    expect(formatResetDuration(3 * 3_600_000 + 54 * 60_000)).toBe("3h 54m");
    expect(formatResetDuration(3 * 3_600_000)).toBe("3h");
    expect(formatResetDuration(6 * 86_400_000 + 7 * 3_600_000)).toBe("6d 7h");
    expect(formatResetDuration(2 * 86_400_000)).toBe("2d");
  });

  it("reports an expired window as now", () => {
    expect(formatResetDuration(0)).toBe("now");
    expect(formatResetDuration(-1_000)).toBe("now");
  });
});

describe("formatResetCountdown", () => {
  it("prefixes remaining time", () => {
    expect(formatResetCountdown(2 * 3_600_000 + 33 * 60_000)).toBe(
      "Resets in 2h 33m",
    );
    expect(formatResetCountdown(0)).toBe("Resets now");
  });
});

describe("formatRateLimitWindowChipLabel", () => {
  const now = Date.parse("2026-08-27T08:00:00Z");

  it("prefers remaining time when resetsAt is known", () => {
    expect(
      formatRateLimitWindowChipLabel(
        {
          usedPercent: 42,
          windowMinutes: 300,
          resetsAt: now + 2 * 3_600_000 + 33 * 60_000,
        },
        now,
      ),
    ).toBe("2h 33m");
  });

  it("falls back to the window size when no reset timestamp exists", () => {
    expect(
      formatRateLimitWindowChipLabel(
        { usedPercent: 42, windowMinutes: 300, resetsAt: null },
        now,
      ),
    ).toBe("5h");
    expect(
      formatRateLimitWindowChipLabel(
        { usedPercent: 41, windowMinutes: 10_080, resetsAt: null },
        now,
      ),
    ).toBe("wk");
  });

  it("prefers an explicit chip label over remaining time", () => {
    expect(
      formatRateLimitWindowChipLabel(
        {
          usedPercent: 32,
          windowMinutes: 44_640,
          resetsAt: now + 6 * 86_400_000,
          chipLabel: "Auto",
        },
        now,
      ),
    ).toBe("Auto");
  });
});

describe("formatUsagePercent", () => {
  it("rounds to a whole percent", () => {
    expect(formatUsagePercent(58.4)).toBe("58%");
    expect(formatUsagePercent(58.6)).toBe("59%");
    expect(clampUsedPercent(140)).toBe(100);
  });
});

describe("parseResetTimestamp", () => {
  it("treats small numbers as unix seconds", () => {
    expect(parseResetTimestamp(1_738_425_600)).toBe(1_738_425_600_000);
  });

  it("keeps millisecond epochs", () => {
    expect(parseResetTimestamp(1_738_425_600_000)).toBe(1_738_425_600_000);
  });

  it("parses ISO strings", () => {
    expect(parseResetTimestamp("2026-08-27T12:00:00.000Z")).toBe(
      Date.parse("2026-08-27T12:00:00.000Z"),
    );
  });
});

describe("parseClaudeOAuthUsage", () => {
  it("maps five_hour and seven_day windows", () => {
    const limits = parseClaudeOAuthUsage(
      JSON.stringify({
        five_hour: { used_percentage: 58.2, resets_at: 1_738_425_600 },
        seven_day: { utilization: 41, resets_at: "2026-09-01T00:00:00.000Z" },
      }),
    );
    expect(limits.status).toBe("ok");
    expect(limits.session).toEqual({
      usedPercent: 58.2,
      windowMinutes: 300,
      resetsAt: 1_738_425_600_000,
    });
    expect(limits.weekly?.usedPercent).toBe(41);
    expect(limits.weekly?.windowMinutes).toBe(10_080);
    expect(limits.weekly?.resetsAt).toBe(
      Date.parse("2026-09-01T00:00:00.000Z"),
    );
  });

  it("returns an error for garbage", () => {
    const limits = parseClaudeOAuthUsage("not json");
    expect(limits.status).toBe("error");
    expect(limits.session).toBeNull();
  });
});

describe("mapUsageWindow", () => {
  it("accepts camelCase Codex-shaped windows", () => {
    expect(
      mapUsageWindow({ usedPercent: 12, resetsAt: 1_738_425_600 }, 300),
    ).toEqual({
      usedPercent: 12,
      windowMinutes: 300,
      resetsAt: 1_738_425_600_000,
    });
  });
});

describe("parseCodexRateLimits", () => {
  it("classifies primary/secondary by duration", () => {
    const limits = parseCodexRateLimits({
      rateLimits: {
        primary: {
          usedPercent: 52,
          windowDurationMins: 300,
          resetsAt: 1_738_425_600,
        },
        secondary: {
          used_percent: 37,
          window_duration_mins: 10_080,
          resets_at: 1_738_900_000,
        },
      },
    });
    expect(limits.session?.usedPercent).toBe(52);
    expect(limits.session?.windowMinutes).toBe(300);
    expect(limits.weekly?.usedPercent).toBe(37);
    expect(limits.weekly?.windowMinutes).toBe(10_080);
  });

  it("falls back to primary=session when durations are unknown", () => {
    const limits = parseCodexRateLimits({
      primary: { usedPercent: 10, resetsAt: 100 },
      secondary: { usedPercent: 20, resetsAt: 200 },
    });
    expect(limits.session?.usedPercent).toBe(10);
    expect(limits.weekly?.usedPercent).toBe(20);
  });
});

describe("rateLimitWindowTooltip", () => {
  it("includes used percent and remaining time", () => {
    const now = Date.parse("2026-08-27T08:00:00Z");
    expect(
      rateLimitWindowTooltip(
        {
          usedPercent: 42.4,
          windowMinutes: 300,
          resetsAt: now + 2 * 3_600_000 + 33 * 60_000,
        },
        now,
      ),
    ).toBe("42% used · Resets in 2h 33m");
  });

  it("prefixes Cursor lane labels", () => {
    const now = Date.parse("2026-08-27T08:00:00Z");
    expect(
      rateLimitWindowTooltip(
        {
          usedPercent: 31.9,
          windowMinutes: 44_640,
          resetsAt: now + 6 * 86_400_000,
          chipLabel: "Auto",
        },
        now,
      ),
    ).toBe("Auto · 32% used · Resets in 6d");
  });
});

describe("sharedWindowResetLabel", () => {
  const now = Date.parse("2026-08-27T08:00:00Z");
  const resetsAt = now + 6 * 86_400_000;

  it("appends one countdown when labeled lanes share a reset", () => {
    expect(
      sharedWindowResetLabel(
        [
          {
            usedPercent: 32,
            windowMinutes: 44_640,
            resetsAt,
            chipLabel: "Auto",
          },
          {
            usedPercent: 45,
            windowMinutes: 44_640,
            resetsAt,
            chipLabel: "API",
          },
        ],
        now,
      ),
    ).toBe("6d");
  });

  it("stays off when the chip already shows remaining time", () => {
    expect(
      sharedWindowResetLabel(
        [
          {
            usedPercent: 32,
            windowMinutes: 300,
            resetsAt,
          },
        ],
        now,
      ),
    ).toBeNull();
  });
});

describe("shouldFetchRateLimits", () => {
  const now = Date.parse("2026-08-27T08:00:00Z");
  const fresh = {
    ...idleRateLimits("claude"),
    status: "ok" as const,
    updatedAt: now - 60_000,
  };
  const stale = {
    ...fresh,
    provider: "codex" as const,
    updatedAt: now - RATE_LIMIT_MIN_REFETCH_MS,
  };

  it("always fetches when forced", () => {
    expect(
      shouldFetchRateLimits({
        force: true,
        visible: false,
        claude: fresh,
        codex: fresh,
        now,
      }),
    ).toBe(true);
  });

  it("skips when the window is hidden", () => {
    expect(
      shouldFetchRateLimits({
        visible: false,
        claude: stale,
        codex: stale,
        now,
      }),
    ).toBe(false);
  });

  it("skips a visible window when both snapshots are fresh", () => {
    expect(
      shouldFetchRateLimits({
        visible: true,
        claude: fresh,
        codex: { ...fresh, provider: "codex" },
        now,
      }),
    ).toBe(false);
  });

  it("fetches on focus once either snapshot is 5 minutes old", () => {
    expect(
      shouldFetchRateLimits({
        visible: true,
        claude: fresh,
        codex: stale,
        now,
      }),
    ).toBe(true);
  });

  it("treats the first idle load as stale", () => {
    expect(isRateLimitSnapshotStale(idleRateLimits("claude"), now)).toBe(true);
  });

  it("does not keep polling a provider that is not connected", () => {
    const disconnected = {
      ...idleRateLimits("codex"),
      status: "unavailable" as const,
      updatedAt: now - RATE_LIMIT_MIN_REFETCH_MS,
      error: "Codex CLI not found",
    };
    expect(isRateLimitSnapshotStale(disconnected, now)).toBe(false);
    expect(
      shouldFetchProvider(disconnected, { visible: true, now }),
    ).toBe(false);
    expect(
      shouldFetchRateLimits({
        visible: true,
        claude: disconnected,
        codex: disconnected,
        now,
      }),
    ).toBe(false);
  });

  it("still polls the connected provider when the other is not", () => {
    const disconnected = {
      ...idleRateLimits("claude"),
      status: "unavailable" as const,
      updatedAt: now - RATE_LIMIT_MIN_REFETCH_MS,
      error: "Claude not signed in",
    };
    expect(
      shouldFetchProvider(disconnected, { visible: true, now }),
    ).toBe(false);
    expect(
      shouldFetchRateLimits({
        visible: true,
        claude: disconnected,
        codex: stale,
        now,
      }),
    ).toBe(true);
  });

  it("retries a disconnected provider only when forced", () => {
    const disconnected = {
      ...idleRateLimits("codex"),
      status: "unavailable" as const,
      updatedAt: now,
      error: "Codex not signed in",
    };
    expect(
      shouldFetchProvider(disconnected, { force: true, visible: true, now }),
    ).toBe(true);
  });
});

describe("parseCursorUsageSummary", () => {
  const cycle = {
    billingCycleStart: "2026-08-14T12:56:47.000Z",
    billingCycleEnd: "2026-09-14T12:56:47.000Z",
  };
  const windowMinutes = Math.round(
    (Date.parse(cycle.billingCycleEnd) - Date.parse(cycle.billingCycleStart)) /
      60_000,
  );

  it("maps Auto and API pools when both percents are present", () => {
    const limits = parseCursorUsageSummary(
      JSON.stringify({
        ...cycle,
        individualUsage: {
          plan: {
            used: 40000,
            limit: 40000,
            autoPercentUsed: 31.96,
            apiPercentUsed: 44.67,
            totalPercentUsed: 33.77,
          },
        },
      }),
    );
    expect(limits.status).toBe("ok");
    expect(limits.provider).toBe("cursor");
    expect(limits.session).toEqual({
      usedPercent: 31.96,
      windowMinutes,
      resetsAt: Date.parse(cycle.billingCycleEnd),
      chipLabel: "Auto",
    });
    expect(limits.weekly).toEqual({
      usedPercent: 44.67,
      windowMinutes,
      resetsAt: Date.parse(cycle.billingCycleEnd),
      chipLabel: "API",
    });
  });

  it("falls back to total plan percent when Auto/API are missing", () => {
    const limits = parseCursorUsageSummary(
      JSON.stringify({
        ...cycle,
        individualUsage: {
          plan: { totalPercentUsed: 18.2 },
        },
      }),
    );
    expect(limits.session).toEqual({
      usedPercent: 18.2,
      windowMinutes,
      resetsAt: Date.parse(cycle.billingCycleEnd),
    });
    expect(limits.weekly).toBeNull();
  });

  it("uses used/limit when Cursor omits percent fields", () => {
    const limits = parseCursorUsageSummary(
      JSON.stringify({
        ...cycle,
        individualUsage: {
          overall: { used: 25, limit: 100 },
        },
      }),
    );
    expect(limits.session?.usedPercent).toBe(25);
    expect(limits.weekly).toBeNull();
  });

  it("returns an error for garbage", () => {
    const limits = parseCursorUsageSummary("not json");
    expect(limits.status).toBe("error");
    expect(limits.session).toBeNull();
  });
});

describe("parseGrokBilling", () => {
  const cycle = {
    type: "USAGE_PERIOD_TYPE_WEEKLY",
    start: "2026-09-03T16:48:05.298690+00:00",
    end: "2026-09-10T16:48:05.298690+00:00",
  };

  it("maps credit usage and the weekly billing period", () => {
    const limits = parseGrokBilling(
      JSON.stringify({
        config: {
          currentPeriod: cycle,
          creditUsagePercent: 19,
          productUsage: [
            { product: "GrokBuild", usagePercent: 17 },
            { product: "GrokChat", usagePercent: 2 },
          ],
        },
      }),
    );
    expect(limits.status).toBe("ok");
    expect(limits.provider).toBe("grok");
    expect(limits.session).toEqual({
      usedPercent: 19,
      windowMinutes: 10_080,
      resetsAt: Date.parse(cycle.end),
    });
    expect(limits.weekly).toBeNull();
  });

  it("falls back to GrokBuild when the headline percent is missing", () => {
    const limits = parseGrokBilling(
      JSON.stringify({
        config: {
          currentPeriod: cycle,
          productUsage: [{ product: "GrokBuild", usagePercent: 17 }],
        },
      }),
    );
    expect(limits.session?.usedPercent).toBe(17);
  });

  it("returns an error for garbage", () => {
    const limits = parseGrokBilling("not json");
    expect(limits.status).toBe("error");
    expect(limits.session).toBeNull();
  });
});

describe("usageFooterProviders", () => {
  it("mirrors the active session by default", () => {
    expect(
      usageFooterProviders({ activeHarness: "claude", alwaysShow: false }),
    ).toEqual(["claude"]);
    expect(
      usageFooterProviders({ activeHarness: "codex", alwaysShow: false }),
    ).toEqual(["codex"]);
    expect(
      usageFooterProviders({ activeHarness: "cursor", alwaysShow: false }),
    ).toEqual(["cursor"]);
    expect(
      usageFooterProviders({ activeHarness: "grok", alwaysShow: false }),
    ).toEqual(["grok"]);
  });

  it("shows nothing by default for a provider without usage data", () => {
    expect(
      usageFooterProviders({ activeHarness: "opencode", alwaysShow: false }),
    ).toEqual([]);
    expect(
      usageFooterProviders({ activeHarness: undefined, alwaysShow: false }),
    ).toEqual([]);
    expect(
      usageFooterProviders({ activeHarness: null, alwaysShow: false }),
    ).toEqual([]);
  });

  it("pins the full roster once the setting is on", () => {
    expect(
      usageFooterProviders({ activeHarness: "opencode", alwaysShow: true }),
    ).toEqual(["claude", "codex", "cursor", "grok"]);
    expect(
      usageFooterProviders({ activeHarness: "claude", alwaysShow: true }),
    ).toEqual(["claude", "codex", "cursor", "grok"]);
    expect(
      usageFooterProviders({ activeHarness: undefined, alwaysShow: true }),
    ).toEqual(["claude", "codex", "cursor", "grok"]);
  });

  it("hands back a fresh array so callers cannot mutate the roster", () => {
    const first = usageFooterProviders({ alwaysShow: true });
    first.pop();
    expect(usageFooterProviders({ alwaysShow: true })).toEqual([
      "claude",
      "codex",
      "cursor",
      "grok",
    ]);
  });

  it("recognises only the providers we can actually poll", () => {
    expect(isRateLimitProvider("claude")).toBe(true);
    expect(isRateLimitProvider("codex")).toBe(true);
    expect(isRateLimitProvider("cursor")).toBe(true);
    expect(isRateLimitProvider("grok")).toBe(true);
    expect(isRateLimitProvider("opencode")).toBe(false);
    expect(isRateLimitProvider(undefined)).toBe(false);
  });
});
