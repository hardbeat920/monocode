// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderRateLimits } from "../../providers/model/rateLimits";
import { mobileAccountUsage, mobileUsageSnapshot } from "./mobileAccountUsage";

const desktop = vi.hoisted(() => ({
  load: vi.fn(),
  cached: vi.fn(),
  save: vi.fn(),
  remaining: vi.fn(),
}));
vi.mock("../../providers/model/rateLimitsCache", () => ({
  loadRateLimits: desktop.load,
  getCachedRateLimits: desktop.cached,
  setCachedRateLimits: desktop.save,
}));
vi.mock("../../settings/model/displayPrefs", () => ({
  loadShowRemainingUsage: desktop.remaining,
}));

const now = Date.parse("2026-10-17T12:00:00Z");
const snapshot = (
  overrides: Partial<ProviderRateLimits> = {},
): ProviderRateLimits => ({
  provider: "codex",
  status: "ok",
  updatedAt: now,
  error: null,
  resetCredits: null,
  session: { usedPercent: 85, windowMinutes: 300, resetsAt: now + 3_600_000 },
  weekly: { usedPercent: 40, windowMinutes: 10080, resetsAt: now + 86_400_000 },
  monthly: null,
  ...overrides,
});

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  localStorage.setItem(
    "monocode.providerAccounts.v1",
    JSON.stringify({
      codex: [{ id: "account-work", provider: "codex", label: "Work" }],
    }),
  );
  desktop.load.mockResolvedValue(snapshot());
  desktop.cached.mockReturnValue(snapshot());
  desktop.remaining.mockReturnValue(false);
});

describe("mobile account usage", () => {
  it("uses the footer's percentages, status, reset times and remaining preference", () => {
    const usage = mobileUsageSnapshot(snapshot(), now, false);
    expect(usage.status).toEqual({
      tone: "low",
      label: "Running low",
      detail: "15% left",
    });
    expect(usage.meters).toEqual([
      {
        title: "5h",
        usedPercent: 85,
        percent: 85,
        label: "85% used",
        reset: "Resets in 1h",
      },
      {
        title: "Weekly",
        usedPercent: 40,
        percent: 40,
        label: "40% used",
        reset: "Resets in 1d",
      },
    ]);
    expect(mobileUsageSnapshot(snapshot(), now, true).meters[0]).toMatchObject({
      percent: 15,
      label: "15% left",
    });
  });

  it("shows exhausted, monthly and unavailable accounts without inventing windows", () => {
    const monthly = mobileUsageSnapshot(
      snapshot({
        session: null,
        weekly: null,
        monthly: {
          usedPercent: 100,
          windowMinutes: 43200,
          resetsAt: now + 86_400_000,
        },
      }),
      now,
      false,
    );
    expect(monthly.status).toMatchObject({
      tone: "exhausted",
      detail: "back in 1d",
    });
    expect(monthly.meters[0].title).toBe("Monthly");
    const missing = mobileUsageSnapshot(
      snapshot({
        session: null,
        weekly: null,
        status: "unavailable",
        error: "Not signed in",
      }),
      now,
      false,
    );
    expect(missing.meters).toEqual([]);
    expect(missing.status.label).toBe("Not signed in");
  });

  it("loads only the requested provider's profiles and forces manual refreshes", async () => {
    await mobileAccountUsage("codex");
    expect(desktop.load.mock.calls).toEqual([
      ["codex", "default", false],
      ["codex", "account-work", false],
    ]);
    desktop.load.mockClear();
    desktop.remaining.mockReturnValue(true);
    const refreshed = await mobileAccountUsage("claude", true);
    expect(desktop.load.mock.calls).toEqual([["claude", "default", true]]);
    expect(refreshed.accounts.default.meters[0].label).toBe("15% left");
    await expect(mobileAccountUsage("pi", true)).rejects.toThrow(
      "only available",
    );
  });

  it("retains the last available meters when a refresh fails", async () => {
    desktop.load.mockResolvedValue(
      snapshot({
        status: "error",
        error: "Network error",
        session: null,
        weekly: null,
      }),
    );
    const refreshed = await mobileAccountUsage("codex", true);
    expect(refreshed.accounts.default.stale).toBe(true);
    expect(refreshed.accounts.default.meters[0].label).toBe("85% used");
    expect(desktop.save).toHaveBeenCalledWith(
      "codex",
      "default",
      expect.objectContaining({ status: "error", session: snapshot().session }),
    );
  });
});
