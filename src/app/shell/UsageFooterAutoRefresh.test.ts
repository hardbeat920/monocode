// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderRateLimits } from "../../features/providers/model/rateLimits";
import {
  clearCachedRateLimits,
  setCachedRateLimits,
  getCachedRateLimits,
  scheduleRateLimitsAutoRefresh,
} from "../../features/providers/model/rateLimitsCache";
import {
  DEFAULT_PROVIDER_ACCOUNT_ID,
  saveProviderAccount,
} from "../../features/providers/model/providerAccounts";
import { newSession, type Session } from "../../features/sessions/model/session";
import { useTurnRateLimitsRefresh } from "../../features/sessions/hooks/useTurnRateLimitsRefresh";
import { UsageFooter } from "./UsageFooter";

const fetches = vi.hoisted(() => ({
  claude: vi.fn<(accountId: string) => Promise<ProviderRateLimits>>(),
  codex: vi.fn<(accountId: string) => Promise<ProviderRateLimits>>(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));
vi.mock("../../features/providers/model/rateLimitsFetch", () => ({
  fetchClaudeRateLimits: fetches.claude,
  fetchCodexRateLimits: fetches.codex,
  fetchDevinRateLimits: vi.fn(),
  fetchOpencodeGoRateLimits: vi.fn(),
  consumeCodexRateLimitResetCredit: vi.fn(),
}));

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

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);

  saveProviderAccount({
    id: DEFAULT_PROVIDER_ACCOUNT_ID,
    provider: "claude",
    label: "Personal",
  });
  setCachedRateLimits("claude", DEFAULT_PROVIDER_ACCOUNT_ID, sampleLimits(10));
  fetches.claude.mockReset();
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  clearCachedRateLimits();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function AppSim({
  session,
  busySessionIds,
}: {
  session: Session;
  busySessionIds: ReadonlySet<string>;
}) {
  useTurnRateLimitsRefresh([session], busySessionIds);
  return createElement(UsageFooter, {
    providers: ["claude"],
    session: {
      id: session.id,
      harness: session.harness,
      model: session.model,
      providerAccountId: session.providerAccountId,
    },
    project: session.cwd,
  });
}

describe("UsageFooter automatic rate-limit refresh", () => {
  it("automatically refreshes usage when an agent turn completes without requiring a button click", async () => {
    fetches.claude.mockResolvedValue(sampleLimits(35));

    const session = {
      ...newSession("claude", "/repo"),
      id: "sess-1",
      providerAccountId: DEFAULT_PROVIDER_ACCOUNT_ID,
    };

    // 1. Initial render while agent is busy
    await act(async () => {
      root.render(
        createElement(AppSim, {
          session,
          busySessionIds: new Set(["sess-1"]),
        }),
      );
    });

    expect(container.textContent).toContain("10%");

    // 2. Turn finishes: busySessionIds clears
    await act(async () => {
      root.render(
        createElement(AppSim, {
          session,
          busySessionIds: new Set(),
        }),
      );
    });

    // Advance past debounce timer
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1600);
    });

    expect(fetches.claude).toHaveBeenCalledTimes(1);
    expect(fetches.claude).toHaveBeenCalledWith(DEFAULT_PROVIDER_ACCOUNT_ID);
    // UI should reflect updated 35% without clicking the refresh button
    expect(container.textContent).toContain("35%");
  });

  it("coalesces clustered completions so rapid turns share a single refresh request", async () => {
    fetches.claude.mockResolvedValue(sampleLimits(50));

    const session = {
      ...newSession("claude", "/repo"),
      id: "sess-1",
      providerAccountId: DEFAULT_PROVIDER_ACCOUNT_ID,
    };

    await act(async () => {
      root.render(
        createElement(AppSim, {
          session,
          busySessionIds: new Set(["sess-1"]),
        }),
      );
    });

    // Rapid completions triggered in quick succession
    scheduleRateLimitsAutoRefresh("claude", DEFAULT_PROVIDER_ACCOUNT_ID, 500);
    await vi.advanceTimersByTimeAsync(200);
    scheduleRateLimitsAutoRefresh("claude", DEFAULT_PROVIDER_ACCOUNT_ID, 500);
    await vi.advanceTimersByTimeAsync(200);
    scheduleRateLimitsAutoRefresh("claude", DEFAULT_PROVIDER_ACCOUNT_ID, 500);

    expect(fetches.claude).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(600);
    });

    expect(fetches.claude).toHaveBeenCalledTimes(1);
  });

  it("refreshes stale usage upon mount when returning to the footer", async () => {
    fetches.claude.mockResolvedValue(sampleLimits(80));

    // Seed with stale data (> 5 minutes old)
    const staleTime = Date.now() - 6 * 60_000;
    setCachedRateLimits(
      "claude",
      DEFAULT_PROVIDER_ACCOUNT_ID,
      sampleLimits(10, staleTime),
    );

    const session = {
      ...newSession("claude", "/repo"),
      id: "sess-1",
      providerAccountId: DEFAULT_PROVIDER_ACCOUNT_ID,
    };

    // Mount should immediately check staleness and refresh without waiting for a focus event
    await act(async () => {
      root.render(
        createElement(UsageFooter, {
          providers: ["claude"],
          session: {
            id: session.id,
            harness: session.harness,
            model: session.model,
            providerAccountId: session.providerAccountId,
          },
          project: session.cwd,
        }),
      );
    });

    expect(fetches.claude).toHaveBeenCalledTimes(1);
    expect(fetches.claude).toHaveBeenCalledWith(DEFAULT_PROVIDER_ACCOUNT_ID);
  });

  it("refreshes stale usage when window receives focus after data ages", async () => {
    fetches.claude.mockResolvedValue(sampleLimits(80));

    // Seed with fresh data
    const freshTime = Date.now();
    setCachedRateLimits(
      "claude",
      DEFAULT_PROVIDER_ACCOUNT_ID,
      sampleLimits(10, freshTime),
    );

    const session = {
      ...newSession("claude", "/repo"),
      id: "sess-1",
      providerAccountId: DEFAULT_PROVIDER_ACCOUNT_ID,
    };

    await act(async () => {
      root.render(
        createElement(UsageFooter, {
          providers: ["claude"],
          session: {
            id: session.id,
            harness: session.harness,
            model: session.model,
            providerAccountId: session.providerAccountId,
          },
          project: session.cwd,
        }),
      );
    });

    expect(fetches.claude).not.toHaveBeenCalled();

    // Age the snapshot by 6 minutes and trigger focus
    await act(async () => {
      await vi.advanceTimersByTimeAsync(6 * 60_000);
      window.dispatchEvent(new Event("focus"));
    });

    expect(fetches.claude).toHaveBeenCalledTimes(1);
    expect(fetches.claude).toHaveBeenCalledWith(DEFAULT_PROVIDER_ACCOUNT_ID);
  });

  it("retries an aged error snapshot on focus or return", async () => {
    fetches.claude.mockResolvedValue(sampleLimits(60));

    // Seed with an error snapshot that is 6 minutes old
    const staleTime = Date.now() - 6 * 60_000;
    const errorSnapshot: ProviderRateLimits = {
      provider: "claude",
      session: null,
      weekly: null,
      monthly: null,
      resetCredits: null,
      updatedAt: staleTime,
      error: "Network timeout",
      status: "error",
    };
    setCachedRateLimits("claude", DEFAULT_PROVIDER_ACCOUNT_ID, errorSnapshot);

    const session = {
      ...newSession("claude", "/repo"),
      id: "sess-1",
      providerAccountId: DEFAULT_PROVIDER_ACCOUNT_ID,
    };

    // Mount checks return and retries the aged error snapshot
    await act(async () => {
      root.render(
        createElement(UsageFooter, {
          providers: ["claude"],
          session: {
            id: session.id,
            harness: session.harness,
            model: session.model,
            providerAccountId: session.providerAccountId,
          },
          project: session.cwd,
        }),
      );
    });

    expect(fetches.claude).toHaveBeenCalledTimes(1);
    expect(fetches.claude).toHaveBeenCalledWith(DEFAULT_PROVIDER_ACCOUNT_ID);
  });
});

