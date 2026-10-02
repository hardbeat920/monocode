import { describe, expect, it } from "vitest";
import { parseClaudeResetCredits } from "./claudeResetCredits";
import { parseClaudeOAuthUsage } from "./rateLimits";

const now = Date.parse("2026-10-02T12:00:00Z");
const grant = {
  id: "promo_1",
  resets_left: 2,
  usable_now: true,
  paused: false,
  clears: ["five_hour", "seven_day"],
  ends_at: "2026-10-22T00:00:00Z",
};
function usage(overrides: Record<string, unknown> = {}) {
  return {
    cedar_ember: {
      eligible: true,
      next_grant_id: "promo_1",
      grants: [grant],
      ...overrides,
    },
  };
}

describe("Claude reset offers", () => {
  it("parses a full grant's count, expiry and scope with a distinct redemption id", () => {
    const result = parseClaudeResetCredits(usage(), now)!;
    expect(result.availableCount).toBe(2);
    expect(result.credits).toHaveLength(1);
    expect(result.credits![0]).toMatchObject({
      id: "cedar_ember:promo_1",
      resetType: "claudeCedar",
      status: "available",
      remainingCount: 2,
      title: "Full reset",
      expiresAt: Date.parse(grant.ends_at),
    });
    expect(result.credits![0].description).toContain(
      "5-hour session limit and weekly limit",
    );
  });
  it("preserves saved but currently unusable grants without making them redeemable", () => {
    for (const [overrides, reason] of [
      [{ next_grant_id: "other" }, "Another reset"],
      [{ cooldown_until: "2026-10-03T00:00:00Z" }, "cooling down"],
      [{ cooldown_until: "invalid" }, "check this offer's cooldown"],
      [
        { eligible: false, ineligible_reason: "surface" },
        "Claude Web or Desktop",
      ],
      [{ grants: [{ ...grant, paused: true }] }, "paused"],
      [
        { grants: [{ ...grant, usable_now: false, use_requires_limit: true }] },
        "covered usage limit",
      ],
    ] as const) {
      const result = parseClaudeResetCredits(usage(overrides), now)!;
      expect(result.availableCount).toBe(2);
      expect(result.credits![0].status).toBe("unavailable");
      expect(result.credits![0].unavailableReason).toContain(reason);
    }
  });
  it("drops spent, expired, malformed and duplicate offers", () => {
    const result = parseClaudeResetCredits(
      usage({
        grants: [
          grant,
          grant,
          { ...grant, id: "spent", resets_left: 0 },
          { ...grant, id: "expired", ends_at: "2026-10-01T00:00:00Z" },
          { ...grant, id: "bad", ends_at: "invalid" },
          { ...grant, id: "../bad" },
          { ...grant, id: "count", resets_left: "2" },
          { ...grant, id: "empty", clears: [] },
          null,
        ],
      }),
      now,
    )!;
    expect(result.availableCount).toBe(2);
    expect(result.credits).toHaveLength(1);
  });
  it("does not invent offers from null placeholders or a Juniper control arm", () => {
    expect(
      parseClaudeResetCredits({ cedar_ember: null, juniper_tide: null }, now),
    ).toBeNull();
    expect(
      parseClaudeResetCredits(
        { juniper_tide: { eligible: true, arm: "control", available: true } },
        now,
      )?.availableCount,
    ).toBe(0);
  });
  it("keeps session-only offers distinct from full resets", () => {
    const result = parseClaudeResetCredits(
      {
        ...usage(),
        juniper_tide: {
          eligible: true,
          arm: "reset",
          available: true,
          weekly_resets_at: grant.ends_at,
        },
      },
      now,
    )!;
    expect(result.availableCount).toBe(3);
    expect(result.credits![1]).toMatchObject({
      id: "juniper_tide",
      title: "5-hour reset",
      resetType: "claudeJuniper",
    });
    expect(result.credits![1].description).toContain(
      "Weekly limits stay unchanged",
    );
  });
  it("reports web-only and failed-discovery states explicitly", () => {
    expect(
      parseClaudeResetCredits(
        usage({ eligible: false, ineligible_reason: "surface", grants: [] }),
        now,
      ),
    ).toMatchObject({
      availableCount: 0,
      notice: expect.stringContaining("Claude Web or Desktop"),
    });
    expect(
      parseClaudeResetCredits({ reset_discovery_failed: true }, now),
    ).toMatchObject({ notice: expect.stringContaining("Could not check") });
  });
  it("includes evaluated reset inventory in the regular Claude usage parser", () => {
    const result = parseClaudeOAuthUsage(
      JSON.stringify({
        cedar_ember: {
          eligible: true,
          next_grant_id: "promo_1",
          grants: [{ ...grant, ends_at: "2099-01-01T00:00:00Z" }],
        },
      }),
    );
    expect(result.resetCredits?.availableCount).toBe(2);
  });
});
