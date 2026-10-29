// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderRateLimits } from "./rateLimits";

const fetches = vi.hoisted(() => ({
  claude: vi.fn<(accountId: string) => Promise<ProviderRateLimits>>(),
  codex: vi.fn<(accountId: string) => Promise<ProviderRateLimits>>(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./rateLimitsFetch", () => ({
  fetchClaudeRateLimits: fetches.claude,
  fetchCodexRateLimits: fetches.codex,
  fetchDevinRateLimits: vi.fn(),
  fetchOpencodeGoRateLimits: vi.fn(),
}));

import {
  clearCachedRateLimits,
  getCachedRateLimits,
  loadRateLimits,
  scheduleRateLimitsAutoRefresh,
  cancelRateLimitsAutoRefresh,
  AUTO_REFRESH_DEBOUNCE_MS,
} from "./rateLimitsCache";

function sampleLimits(
  usedPercent: number,
  updatedAt = Date.now(),
): ProviderRateLimits {
  return {
    provider: "claude",
    session: { usedPercent, windowMinutes: 300, resetsAt: Date.now() + 3600000 },
    weekly: null,
    monthly: null,
    resetCredits: null,
    updatedAt,
    error: null,
    status: "ok",
  };
}

describe("rateLimitsCache auto-refresh debouncing", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    clearCachedRateLimits();
    fetches.claude.mockReset();
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
    clearCachedRateLimits();
  });

  it("debounces rapid triggers and combines clustered completions into a single refresh", async () => {
    fetches.claude.mockResolvedValue(sampleLimits(42));

    // Multiple rapid turn completions within the debounce window
    scheduleRateLimitsAutoRefresh("claude", "default", 500);
    vi.advanceTimersByTime(200);
    scheduleRateLimitsAutoRefresh("claude", "default", 500);
    vi.advanceTimersByTime(200);
    scheduleRateLimitsAutoRefresh("claude", "default", 500);

    expect(fetches.claude).not.toHaveBeenCalled();

    // Advance past the final debounce timeout
    await vi.advanceTimersByTimeAsync(500);

    expect(fetches.claude).toHaveBeenCalledTimes(1);
    expect(fetches.claude).toHaveBeenCalledWith("default");
    expect(getCachedRateLimits("claude", "default").session?.usedPercent).toBe(42);
  });

  it("handles distinct accounts with their own debounce timers", async () => {
    fetches.claude.mockResolvedValue(sampleLimits(10));

    scheduleRateLimitsAutoRefresh("claude", "account-1", 500);
    scheduleRateLimitsAutoRefresh("claude", "account-2", 500);

    await vi.advanceTimersByTimeAsync(500);

    expect(fetches.claude).toHaveBeenCalledTimes(2);
    expect(fetches.claude).toHaveBeenCalledWith("account-1");
    expect(fetches.claude).toHaveBeenCalledWith("account-2");
  });

  it("can cancel pending auto-refresh timers", async () => {
    fetches.claude.mockResolvedValue(sampleLimits(10));

    const cancel = scheduleRateLimitsAutoRefresh("claude", "default", 500);
    cancel();

    await vi.advanceTimersByTimeAsync(1000);

    expect(fetches.claude).not.toHaveBeenCalled();
  });

  it("cancels a scheduled auto-refresh when an intervening forced load starts", async () => {
    fetches.claude.mockResolvedValue(sampleLimits(15));

    // Schedule auto-refresh (e.g. from turn completion)
    scheduleRateLimitsAutoRefresh("claude", "default", 1000);

    // User or focus triggers a forced load before debounce timeout fires
    await vi.advanceTimersByTimeAsync(300);
    await loadRateLimits("claude", "default", true);

    expect(fetches.claude).toHaveBeenCalledTimes(1);

    // Debounce window completes - timer should have been canceled, so no second load
    await vi.advanceTimersByTimeAsync(1500);

    expect(fetches.claude).toHaveBeenCalledTimes(1);
  });

  it("skips auto-refresh callback if an intervening load already updated the snapshot", async () => {
    fetches.claude.mockResolvedValue(sampleLimits(20));

    scheduleRateLimitsAutoRefresh("claude", "default", 1000);

    // Another load updates the cache at a later timestamp
    vi.setSystemTime(Date.now() + 200);
    fetches.claude.mockResolvedValueOnce(sampleLimits(50, Date.now()));
    await loadRateLimits("claude", "default", true);

    expect(fetches.claude).toHaveBeenCalledTimes(1);

    // Advance past original debounce timer
    await vi.advanceTimersByTimeAsync(1000);

    // Callback should not trigger a redundant load
    expect(fetches.claude).toHaveBeenCalledTimes(1);
  });
});

