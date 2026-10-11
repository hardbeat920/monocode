import { describe, expect, it } from "vitest";
import {
  activeRateLimitPool,
  clampUsedPercent,
  exhaustedWindowResetAt,
  formatRateLimitWindowChipLabel,
  formatResetCountdown,
  formatResetDuration,
  formatUsagePercent,
  formatWindowLabel,
  idleRateLimits,
  mapUsageWindow,
  parseAntigravityUsage,
  parseClaudeOAuthUsage,
  parseCodexRateLimits,
  formatExtraUsageBalance,
  parseDevinUsage,
  parseOpencodeGoUsage,
  parseResetTimestamp,
  rateLimitWindowTooltip,
} from "./rateLimits";

describe("parseDevinUsage", () => {
  it("turns remaining daily and weekly quota into used windows", () => {
    const limits = parseDevinUsage({
      plan: "Pro",
      dailyRemainingPercent: 0,
      dailyResetsAt: 1_791_273_600,
      weeklyRemainingPercent: 42,
      weeklyResetsAt: 1_791_705_600,
      extraUsageBalanceMicros: -1_673_099,
    });
    expect(formatExtraUsageBalance(limits.extraUsageBalance!)).toBe("$-1.67");
    expect(limits).toMatchObject({
      provider: "devin",
      status: "ok",
      session: { usedPercent: 100, windowMinutes: 1_440, resetsAt: 1_791_273_600_000 },
      weekly: { usedPercent: 58, windowMinutes: 10_080, resetsAt: 1_791_705_600_000 },
      monthly: null,
    });
  });

  it("leaves out windows the plan hides", () => {
    const limits = parseDevinUsage({ weeklyRemainingPercent: 80, weeklyResetsAt: null });
    expect(limits.session).toBeNull();
    expect(limits.weekly).toEqual({ usedPercent: 20, windowMinutes: 10_080, resetsAt: null });
    expect(limits.extraUsageBalance).toBeNull();
  });
});

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
});

describe("formatUsagePercent", () => {
  it("rounds to a whole percent", () => {
    expect(formatUsagePercent(58.4)).toBe("58%");
    expect(formatUsagePercent(58.6)).toBe("59%");
    expect(clampUsedPercent(140)).toBe(100);
  });
});

describe("exhaustedWindowResetAt", () => {
  it("returns the latest reset among spent windows", () => {
    const limits = {
      ...idleRateLimits("codex"),
      session: { usedPercent: 100, windowMinutes: 300, resetsAt: 2_000 },
      weekly: { usedPercent: 100, windowMinutes: 10_080, resetsAt: 9_000 },
    };
    expect(exhaustedWindowResetAt(limits)).toBe(9_000);
  });

  it("ignores windows with room left", () => {
    const limits = {
      ...idleRateLimits("claude"),
      session: { usedPercent: 100, windowMinutes: 300, resetsAt: 2_000 },
      weekly: { usedPercent: 40, windowMinutes: 10_080, resetsAt: 9_000 },
    };
    expect(exhaustedWindowResetAt(limits)).toBe(2_000);
    expect(exhaustedWindowResetAt(idleRateLimits("claude"))).toBeNull();
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

  it("maps banked reset credits and their expiry", () => {
    const limits = parseCodexRateLimits({
      rateLimits: {
        primary: { usedPercent: 52, windowDurationMins: 300 },
      },
      rateLimitResetCredits: {
        availableCount: 2,
        credits: [
          {
            id: "reset-1",
            resetType: "codexRateLimits",
            status: "available",
            grantedAt: 1_788_768_000,
            expiresAt: 1_791_360_000,
            title: "Referral reward",
            description: "One Codex rate-limit reset",
          },
        ],
      },
    });

    expect(limits.resetCredits).toEqual({
      availableCount: 2,
      credits: [
        {
          id: "reset-1",
          resetType: "codexRateLimits",
          status: "available",
          grantedAt: 1_788_768_000_000,
          expiresAt: 1_791_360_000_000,
          title: "Referral reward",
          description: "One Codex rate-limit reset",
        },
      ],
    });
  });

  it("keeps an aggregate banked reset count without detail rows", () => {
    const limits = parseCodexRateLimits({
      rateLimits: {
        primary: { usedPercent: 12, windowDurationMins: 300 },
      },
      rate_limit_reset_credits: {
        available_count: "3",
        credits: null,
      },
    });

    expect(limits.resetCredits).toEqual({ availableCount: 3, credits: null });
  });

  it("maps a free plan's lone 30-day primary window to monthly", () => {
    const limits = parseCodexRateLimits({
      rateLimits: {
        primary: {
          usedPercent: 4,
          windowDurationMins: 43_200,
          resetsAt: 1_792_550_273,
        },
        secondary: null,
      },
    });
    expect(limits.session).toBeNull();
    expect(limits.weekly).toBeNull();
    expect(limits.monthly).toEqual({
      usedPercent: 4,
      windowMinutes: 43_200,
      resetsAt: 1_792_550_273_000,
    });
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

describe("parseOpencodeGoUsage", () => {
  it("maps rolling/weekly/monthly windows with reset times", () => {
    const limits = parseOpencodeGoUsage({
      usage: {
        rolling: {
          status: "ok",
          percent: 42,
          resetsAt: "2026-09-16T16:27:38.287Z",
        },
        weekly: { status: "ok", percent: 30, resetsAt: "2026-09-23T00:00:00Z" },
        monthly: {
          status: "ok",
          percent: 12,
          resetsAt: "2026-10-16T00:00:00Z",
        },
      },
    });
    expect(limits.provider).toBe("opencode");
    expect(limits.session?.usedPercent).toBe(42);
    expect(limits.session?.windowMinutes).toBe(300);
    expect(limits.weekly?.usedPercent).toBe(30);
    expect(limits.weekly?.windowMinutes).toBe(10_080);
    expect(limits.monthly?.usedPercent).toBe(12);
    expect(limits.monthly?.windowMinutes).toBe(43_200);
    expect(limits.session?.resetsAt).toBe(
      Date.parse("2026-09-16T16:27:38.287Z"),
    );
  });

  it("drops non-ok windows instead of claiming usage", () => {
    const limits = parseOpencodeGoUsage({
      usage: {
        rolling: { status: "ok", percent: 5, resetsAt: null },
        weekly: { status: "expired", percent: 50, resetsAt: null },
        monthly: { percent: 50, resetsAt: null },
      },
    });
    expect(limits.session?.usedPercent).toBe(5);
    expect(limits.weekly).toBeNull();
    expect(limits.monthly).toBeNull();
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

  it("shows remaining percent with a reset countdown", () => {
    const now = Date.parse("2026-08-27T08:00:00Z");
    expect(
      rateLimitWindowTooltip(
        {
          usedPercent: 42.4,
          windowMinutes: 300,
          resetsAt: now + 2 * 3_600_000,
        },
        now,
        true,
      ),
    ).toBe("58% remaining · Resets in 2h");
  });

  it.each([
    [0, "100%"],
    [100, "0%"],
    [-10, "100%"],
    [110, "0%"],
  ])(
    "clamps %s used before showing remaining usage",
    (usedPercent, remaining) => {
      expect(
        rateLimitWindowTooltip(
          { usedPercent, windowMinutes: 10_080, resetsAt: null },
          0,
          true,
        ),
      ).toBe(`${remaining} remaining · wk window`);
    },
  );
});

describe("parseAntigravityUsage", () => {
  const body = JSON.stringify({
    models: {
      "gemini-3.1-pro-high": {
        displayName: "Gemini 3.1 Pro (High)",
        quotaInfo: { remainingFraction: 0.92, resetTime: "2026-10-09T12:16:47Z" },
      },
      "gemini-3-flash": {
        displayName: "Gemini 3 Flash",
        quotaInfo: { remainingFraction: 0.92, resetTime: "2026-10-09T12:16:47Z" },
      },
      // Proto JSON drops a zero remainingFraction: this pool is spent.
      "claude-opus-5-5-high": {
        displayName: "Claude Opus 5.5 (High)",
        quotaInfo: { resetTime: "2026-10-10T12:24:28Z" },
      },
      "gpt-oss-120b-medium": {
        displayName: "GPT-OSS 120B (Medium)",
        quotaInfo: { resetTime: "2026-10-10T12:24:28Z" },
      },
      // Internal models carry no display name.
      chat_20706: { quotaInfo: { remainingFraction: 1 } },
      "gemini-3.6-flash-tiered": {
        quotaInfo: { remainingFraction: 0.92, resetTime: "2026-10-09T12:16:47Z" },
      },
    },
  });

  it("groups models that share a quota and labels them by family", () => {
    const limits = parseAntigravityUsage(body);
    expect(limits.status).toBe("ok");
    expect(limits.session).toBeNull();
    expect(limits.pools).toEqual([
      {
        id: "claude-opus-5-5-high",
        label: "Claude & GPT-OSS",
        shortLabel: "Claude",
        modelIds: ["claude-opus-5-5-high", "gpt-oss-120b-medium"],
        modelNames: ["Claude Opus 5.5 (High)", "GPT-OSS 120B (Medium)"],
        window: {
          usedPercent: 100,
          windowMinutes: 0,
          resetsAt: Date.parse("2026-10-10T12:24:28Z"),
        },
      },
      {
        id: "gemini-3-flash",
        label: "Gemini",
        shortLabel: "Gemini",
        modelIds: ["gemini-3-flash", "gemini-3.1-pro-high"],
        modelNames: ["Gemini 3 Flash", "Gemini 3.1 Pro (High)"],
        window: {
          usedPercent: expect.closeTo(8, 5),
          windowMinutes: 0,
          resetsAt: Date.parse("2026-10-09T12:16:47Z"),
        },
      },
    ]);
    expect(exhaustedWindowResetAt(limits)).toBe(
      Date.parse("2026-10-10T12:24:28Z"),
    );
  });

  it("names split pools within one family by model", () => {
    const limits = parseAntigravityUsage(
      JSON.stringify({
        models: {
          pro: {
            displayName: "Gemini 3.1 Pro (High)",
            quotaInfo: { remainingFraction: 0.5 },
          },
          flash: {
            displayName: "Gemini 3 Flash",
            quotaInfo: { remainingFraction: 1 },
          },
        },
      }),
    );
    expect(limits.pools?.map((pool) => [pool.label, pool.shortLabel])).toEqual([
      ["Gemini 3 Flash", "Gemini 3 Flash"],
      ["Gemini 3.1 Pro", "Gemini 3.1 Pro"],
    ]);
  });

  it("keeps untouched quotas apart by family", () => {
    const limits = parseAntigravityUsage(
      JSON.stringify({
        models: {
          flash: {
            displayName: "Gemini 3 Flash",
            quotaInfo: { remainingFraction: 1 },
          },
          opus: {
            displayName: "Claude Opus 5.5 (High)",
            quotaInfo: { remainingFraction: 1 },
          },
          // No fraction and no reset says nothing about the quota.
          unknown: { displayName: "Gemini 2.5 Pro", quotaInfo: {} },
        },
      }),
    );
    expect(limits.pools?.map((pool) => pool.label)).toEqual([
      "Claude",
      "Gemini",
    ]);
    expect(limits.pools?.every((pool) => pool.window.usedPercent === 0)).toBe(
      true,
    );
  });

  it("keeps models one quota apart when resets differ by seconds", () => {
    const limits = parseAntigravityUsage(
      JSON.stringify({
        models: {
          a: {
            displayName: "Gemini 3 Flash",
            quotaInfo: { remainingFraction: 0.5, resetTime: "2026-10-09T12:16:47Z" },
          },
          b: {
            displayName: "Gemini 3.1 Pro (High)",
            quotaInfo: { remainingFraction: 0.5, resetTime: "2026-10-09T12:16:48Z" },
          },
        },
      }),
    );
    expect(limits.pools).toHaveLength(1);
  });

  it("gives effort variants in separate quotas distinct labels", () => {
    const limits = parseAntigravityUsage(
      JSON.stringify({
        models: {
          high: {
            displayName: "Gemini 3.1 Pro (High)",
            quotaInfo: { remainingFraction: 0.2, resetTime: "2026-10-09T12:00:00Z" },
          },
          low: {
            displayName: "Gemini 3.1 Pro (Low)",
            quotaInfo: { remainingFraction: 0.7, resetTime: "2026-10-09T12:00:00Z" },
          },
        },
      }),
    );
    expect(limits.pools?.map((pool) => [pool.id, pool.label])).toEqual([
      ["high", "Gemini 3.1 Pro (High)"],
      ["low", "Gemini 3.1 Pro (Low)"],
    ]);
  });

  it("reports malformed payloads as errors", () => {
    expect(parseAntigravityUsage("nope").status).toBe("error");
    expect(parseAntigravityUsage("{}").status).toBe("error");
  });

  it("finds the pool serving a model by id or display name", () => {
    const limits = parseAntigravityUsage(body);
    expect(activeRateLimitPool(limits, { id: "gemini-3-flash" })?.label).toBe(
      "Gemini",
    );
    expect(
      activeRateLimitPool(limits, {
        id: "claude-opus",
        name: "Claude Opus 5.5 (High)",
      })?.label,
    ).toBe("Claude & GPT-OSS");
    // ACP names can drop the effort suffix the quota API keeps.
    expect(
      activeRateLimitPool(limits, {
        id: "gemini-3.1-pro",
        name: "Gemini 3.1 Pro",
      })?.label,
    ).toBe("Gemini");
    expect(activeRateLimitPool(limits, { id: "unknown" })).toBeNull();
  });

  it("formats pools without a reset time as bare usage", () => {
    const window = { usedPercent: 0, windowMinutes: 0, resetsAt: null };
    expect(formatRateLimitWindowChipLabel(window, 0)).toBe("");
    expect(rateLimitWindowTooltip(window, 0)).toBe("0% used");
  });
});
