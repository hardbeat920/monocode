import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const fetches = vi.hoisted(() => ({
  claude: vi.fn(),
  codex: vi.fn(),
}));

vi.mock("./rateLimitsFetch", () => ({
  fetchClaudeRateLimits: fetches.claude,
  fetchCodexRateLimits: fetches.codex,
  fetchDevinRateLimits: vi.fn(),
  fetchOpencodeGoRateLimits: vi.fn(),
}));

import {
  applyLiveCodexRateLimits,
  clearCachedRateLimits,
  getCachedRateLimits,
  loadRateLimits,
  refreshRateLimitsIfStale,
} from "./rateLimitsCache";
import {
  errorRateLimits,
  RATE_LIMIT_MIN_REFETCH_MS,
  type ProviderRateLimits,
} from "./rateLimits";

function okClaude(usedPercent: number): ProviderRateLimits {
  return {
    provider: "claude",
    session: { usedPercent, windowMinutes: 300, resetsAt: null },
    weekly: null,
    monthly: null,
    resetCredits: null,
    updatedAt: Date.now(),
    error: null,
    status: "ok",
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000_000);
  clearCachedRateLimits();
  fetches.claude.mockReset();
  fetches.codex.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("loadRateLimits", () => {
  it("keeps the last good windows and their read time when a refresh fails", async () => {
    fetches.claude.mockResolvedValueOnce(okClaude(30));
    await loadRateLimits("claude", "default");
    vi.setSystemTime(2_000_000);
    fetches.claude.mockResolvedValueOnce(errorRateLimits("claude", "offline"));
    const result = await loadRateLimits("claude", "default", true);
    expect(result.status).toBe("error");
    expect(result.error).toBe("offline");
    expect(result.session?.usedPercent).toBe(30);
    expect(result.updatedAt).toBe(1_000_000);
  });
});

describe("applyLiveCodexRateLimits", () => {
  it("publishes a running session's usage for its account", () => {
    applyLiveCodexRateLimits("work", {
      primary: { usedPercent: 55, windowDurationMins: 300 },
    });
    const cached = getCachedRateLimits("codex", "work");
    expect(cached.status).toBe("ok");
    expect(cached.session?.usedPercent).toBe(55);
    expect(getCachedRateLimits("codex", "default").status).toBe("idle");
  });

  it("ignores updates without a usable window", () => {
    applyLiveCodexRateLimits("default", { primary: null });
    expect(getCachedRateLimits("codex", "default").status).toBe("idle");
  });
});

describe("refreshRateLimitsIfStale", () => {
  it("does nothing for an account the footer never loaded", () => {
    refreshRateLimitsIfStale("claude", "default");
    expect(fetches.claude).not.toHaveBeenCalled();
  });

  it("refetches a good reading only after the minimum interval", async () => {
    fetches.claude.mockResolvedValue(okClaude(30));
    await loadRateLimits("claude", "default");
    refreshRateLimitsIfStale("claude", "default");
    expect(fetches.claude).toHaveBeenCalledTimes(1);
    vi.setSystemTime(1_000_000 + RATE_LIMIT_MIN_REFETCH_MS);
    refreshRateLimitsIfStale("claude", "default");
    expect(fetches.claude).toHaveBeenCalledTimes(2);
  });

  it("retries a failed reading after a minute", async () => {
    fetches.claude.mockResolvedValue(errorRateLimits("claude", "stale"));
    await loadRateLimits("claude", "default");
    vi.setSystemTime(1_000_000 + 30_000);
    refreshRateLimitsIfStale("claude", "default");
    expect(fetches.claude).toHaveBeenCalledTimes(1);
    vi.setSystemTime(1_000_000 + 60_000);
    refreshRateLimitsIfStale("claude", "default");
    expect(fetches.claude).toHaveBeenCalledTimes(2);
  });
});
