// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderRateLimits, RateLimitWindow } from "./rateLimits";

const fetches = vi.hoisted(() => ({
  claude: vi.fn<(accountId: string) => Promise<ProviderRateLimits>>(),
  codex: vi.fn<(accountId: string) => Promise<ProviderRateLimits>>(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./rateLimitsFetch", () => ({
  fetchClaudeRateLimits: fetches.claude,
  fetchCodexRateLimits: fetches.codex,
}));

import {
  accountHeadroom,
  accountStatus,
  bestAlternativeAccount,
  useProviderAccountUsage,
  type AccountUsage,
} from "./accountUsage";
import type { ProviderAccount } from "./providerAccounts";

const now = Date.parse("2026-09-25T12:00:00Z");
const HOUR = 3_600_000;

function window(
  usedPercent: number,
  resetsAt: number | null = now + HOUR,
): RateLimitWindow {
  return { usedPercent, windowMinutes: 300, resetsAt };
}

function limits(
  session: RateLimitWindow | null,
  weekly: RateLimitWindow | null = null,
  overrides: Partial<ProviderRateLimits> = {},
): ProviderRateLimits {
  return {
    provider: "claude",
    session,
    weekly,
    monthly: null,
    resetCredits: null,
    updatedAt: now,
    error: null,
    status: "ok",
    ...overrides,
  };
}

function account(id: string): ProviderAccount {
  return { id, provider: "claude", label: id };
}

describe("accountHeadroom", () => {
  it("uses the tightest window", () => {
    expect(accountHeadroom(limits(window(30), window(75)), now)).toBe(25);
  });

  it("treats a window past its reset time as available", () => {
    expect(
      accountHeadroom(limits(window(100, now - 1), window(40)), now),
    ).toBe(60);
  });

  it("is null without usage windows", () => {
    expect(accountHeadroom(limits(null), now)).toBeNull();
    expect(accountHeadroom(undefined, now)).toBeNull();
  });
});

describe("accountStatus", () => {
  it("reads Ready with comfortable headroom", () => {
    expect(accountStatus(limits(window(10), window(50)), now)).toEqual({
      tone: "ready",
      label: "Ready",
      detail: null,
    });
  });

  it("reads Running low at or below 20% headroom", () => {
    expect(accountStatus(limits(window(84)), now)).toEqual({
      tone: "low",
      label: "Running low",
      detail: "16% left",
    });
  });

  it("reads Exhausted with the latest reset of the used-up windows", () => {
    expect(
      accountStatus(
        limits(window(100, now + 31 * 60_000), window(100, now + 2 * HOUR)),
        now,
      ),
    ).toEqual({ tone: "exhausted", label: "Exhausted", detail: "back in 2h" });
  });

  it("reads Checking while the first snapshot loads", () => {
    expect(accountStatus(undefined, now).tone).toBe("checking");
    expect(
      accountStatus(limits(null, null, { status: "fetching" }), now).tone,
    ).toBe("checking");
  });

  it("surfaces why usage is missing", () => {
    expect(
      accountStatus(
        limits(null, null, {
          status: "unavailable",
          error: "Claude not signed in",
        }),
        now,
      ),
    ).toEqual({ tone: "unknown", label: "Claude not signed in", detail: null });
    expect(
      accountStatus(
        limits(null, null, {
          status: "error",
          error: "Claude sign-in expired",
        }),
        now,
      ).label,
    ).toBe("Claude sign-in expired");
  });
});

describe("bestAlternativeAccount", () => {
  it("picks the account with the most headroom above the low threshold", () => {
    const usage: Record<string, ProviderRateLimits> = {
      low: limits(window(85)),
      mid: limits(window(40)),
      best: limits(window(5), window(10)),
    };
    const accounts = ["low", "mid", "best"].map(account);
    expect(
      bestAlternativeAccount(accounts, (entry) => usage[entry.id], now)?.id,
    ).toBe("best");
  });

  it("suggests nothing when every other account is low or unknown", () => {
    const usage: Record<string, ProviderRateLimits | undefined> = {
      low: limits(window(95)),
      unknown: undefined,
    };
    expect(
      bestAlternativeAccount(
        ["low", "unknown"].map(account),
        (entry) => usage[entry.id],
        now,
      ),
    ).toBeNull();
  });
});

describe("useProviderAccountUsage", () => {
  let container: HTMLDivElement;
  let root: Root;
  let latest: AccountUsage;

  function Probe({ enabled }: { enabled: boolean }) {
    latest = useProviderAccountUsage(0, { provider: "claude", enabled });
    return null;
  }

  function render(enabled = true) {
    act(() => root.render(createElement(Probe, { enabled })));
  }

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    localStorage.setItem(
      "monocode.providerAccounts.v1",
      JSON.stringify({
        claude: [{ id: "account-work", provider: "claude", label: "Work" }],
      }),
    );
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    fetches.claude.mockReset();
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    localStorage.clear();
    vi.unstubAllGlobals();
  });

  it("loads every account of the provider once enabled", async () => {
    fetches.claude.mockImplementation(async () => limits(window(12)));
    render(false);
    expect(fetches.claude).not.toHaveBeenCalled();

    await act(async () => render(true));

    expect(fetches.claude.mock.calls.map(([id]) => id).sort()).toEqual([
      "account-work",
      "default",
    ]);
    expect(latest.usage["claude:account-work"]?.session?.usedPercent).toBe(12);
    expect(latest.refreshing).toBe(false);
  });

  it("keeps the newest response when refreshes overlap", async () => {
    const pending: ((value: ProviderRateLimits) => void)[] = [];
    fetches.claude.mockImplementation(
      (id) =>
        id === "default"
          ? new Promise((resolve) => pending.push(resolve))
          : Promise.resolve(limits(window(1))),
    );
    render();
    await act(async () => latest.refresh());
    expect(pending).toHaveLength(2);

    // The refresh answers first; the original load answers late.
    await act(async () => pending[1](limits(window(70))));
    expect(latest.refreshing).toBe(true);
    await act(async () => pending[0](limits(window(10))));

    expect(latest.usage["claude:default"]?.session?.usedPercent).toBe(70);
    expect(latest.refreshing).toBe(false);
  });
});
