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
  setCachedRateLimits,
  updateCodexRateLimits,
} from "./rateLimitsCache";
import { saveProviderAccount, removeProviderAccount } from "./providerAccounts";

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
    fetches.codex.mockReset();
    localStorage.clear();
  });

  it("merges sparse Codex pushes into the originating account without fetching or losing other windows/credits", () => {
    const account = { id: "work-account", provider: "codex" as const, label: "Work" };
    saveProviderAccount(account);
    const cached = { ...sampleLimits(10), provider: "codex" as const,
      weekly: { usedPercent: 25, windowMinutes: 10080, resetsAt: 1_800_000_000_000 },
      resetCredits: { availableCount: 1, credits: null },
    };
    setCachedRateLimits("codex", account.id, cached);
    const update = { limitId: "codex", primary: { usedPercent: 40, windowDurationMins: 300, resetsAt: 1_800_000_000 } };
    updateCodexRateLimits(update, account.id);
    expect(getCachedRateLimits("codex", account.id)).toMatchObject({
      status: "ok", session: { usedPercent: 40 }, weekly: cached.weekly, resetCredits: cached.resetCredits,
    });
    expect(getCachedRateLimits("codex").status).toBe("idle");
    updateCodexRateLimits({ ...update, limitId: "codex_other" }, account.id);
    updateCodexRateLimits({ limitId: "codex" }, account.id);
    expect(getCachedRateLimits("codex", account.id).session?.usedPercent).toBe(40);
    removeProviderAccount("codex", account.id);
    clearCachedRateLimits("codex", account.id);
    updateCodexRateLimits(update, account.id);
    expect(getCachedRateLimits("codex", account.id).status).toBe("idle");
    expect(fetches.codex).not.toHaveBeenCalled();
  });

  it.each([false, true])("keeps a newer Codex push when an older probe settles (error=%s)", async (fails) => {
    let resolve!: (limits: ProviderRateLimits) => void;
    let reject!: (error: Error) => void;
    fetches.codex.mockImplementationOnce(() => new Promise((yes, no) => { resolve = yes; reject = no; }));
    const request = loadRateLimits("codex", "default", true);
    updateCodexRateLimits({ primary: { usedPercent: 45, windowDurationMins: 300 } });
    if (fails) reject(new Error("Old request failed"));
    else resolve({ ...sampleLimits(5), provider: "codex" });
    expect((await request).session?.usedPercent).toBe(45);
    expect(getCachedRateLimits("codex")).toMatchObject({ status: "ok", session: { usedPercent: 45 } });
  });

  it.each([
    ["session", "primary", 300],
    ["weekly", "secondary", 10080],
    ["monthly", "primary", 43200],
  ] as const)("preserves an omitted %s reset time but accepts a new one", (window, slot, windowMinutes) => {
    const resetsAt = 1_800_000_000_000;
    setCachedRateLimits("codex", "default", {
      ...sampleLimits(10),
      provider: "codex",
      [window]: { usedPercent: 10, windowMinutes, resetsAt },
    });

    updateCodexRateLimits({ [slot]: { usedPercent: 45, windowDurationMins: windowMinutes } });
    expect(getCachedRateLimits("codex")[window]).toMatchObject({ usedPercent: 45, resetsAt });

    updateCodexRateLimits({ [slot]: {
      usedPercent: 50, windowDurationMins: windowMinutes, resetsAt: 1_800_100_000,
    } });
    expect(getCachedRateLimits("codex")[window]).toMatchObject({
      usedPercent: 50, resetsAt: 1_800_100_000_000,
    });
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

  it("queues a fresh request for a turn completed after a queued refresh has started", async () => {
    let resolveFirst!: (limits: ProviderRateLimits) => void;
    let resolveSecond!: (limits: ProviderRateLimits) => void;
    fetches.claude
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockImplementationOnce(() => new Promise((resolve) => { resolveSecond = resolve; }))
      .mockResolvedValue(sampleLimits(80));
    const first = loadRateLimits("claude", "default", true);
    const second = loadRateLimits("claude", "default", true);
    expect(loadRateLimits("claude", "default", true)).toBe(second);
    resolveFirst(sampleLimits(20));
    await first;
    expect(fetches.claude).toHaveBeenCalledTimes(2);

    scheduleRateLimitsAutoRefresh("claude", "default", 500);
    await vi.advanceTimersByTimeAsync(500);
    const third = loadRateLimits("claude", "default", true);
    expect(third).not.toBe(second);
    expect(loadRateLimits("claude", "default", true)).toBe(third);
    resolveSecond(sampleLimits(40));
    await Promise.all([second, third]);
    expect(fetches.claude).toHaveBeenCalledTimes(3);
    expect(getCachedRateLimits("claude").session?.usedPercent).toBe(80);
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

  it("refreshes after a pre-turn request resolves during the completion debounce", async () => {
    let resolvePreTurn!: (limits: ProviderRateLimits) => void;
    fetches.claude
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolvePreTurn = resolve;
          }),
      )
      .mockResolvedValue(sampleLimits(75));

    // This request started before the turn completed, so its result may be outdated.
    const preTurn = loadRateLimits("claude", "default", true);
    scheduleRateLimitsAutoRefresh("claude", "default", 1000);

    vi.setSystemTime(Date.now() + 200);
    resolvePreTurn(sampleLimits(50));
    await preTurn;
    expect(getCachedRateLimits("claude", "default").session?.usedPercent).toBe(
      50,
    );

    // A later updatedAt does not prove this snapshot includes the completed turn.
    await vi.advanceTimersByTimeAsync(1000);
    expect(fetches.claude).toHaveBeenCalledTimes(2);
    expect(getCachedRateLimits("claude", "default").session?.usedPercent).toBe(
      75,
    );
  });

  it("retries after a pre-turn request fails during the completion debounce", async () => {
    let rejectPreTurn!: (reason: Error) => void;
    fetches.claude
      .mockImplementationOnce(
        () =>
          new Promise((_, reject) => {
            rejectPreTurn = reject;
          }),
      )
      .mockResolvedValue(sampleLimits(65));

    const preTurn = loadRateLimits("claude", "default", true);
    scheduleRateLimitsAutoRefresh("claude", "default", 1000);

    vi.setSystemTime(Date.now() + 200);
    rejectPreTurn(new Error("Temporary network failure"));
    await preTurn;
    expect(getCachedRateLimits("claude", "default").status).toBe("error");

    // The error has a newer timestamp, but the post-turn refresh is still needed.
    await vi.advanceTimersByTimeAsync(1000);
    expect(fetches.claude).toHaveBeenCalledTimes(2);
    expect(getCachedRateLimits("claude", "default").session?.usedPercent).toBe(
      65,
    );
  });
});
