// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { handleAgentApp, type AgentAppHost } from "./agentApp";
import { usageSnapshot } from "./usageSnapshot";
import { newSession } from "../../sessions/model/session";
import {
  providerAccounts,
  saveProviderAccount,
  selectProviderAccount,
} from "../../providers/model/providerAccounts";
import {
  clearCachedRateLimits,
  getCachedRateLimits,
  loadRateLimits,
  setCachedRateLimits,
} from "../../providers/model/rateLimitsCache";
import {
  errorRateLimits,
  idleRateLimits,
  parseClaudeOAuthUsage,
  parseCodexRateLimits,
  parseOpencodeGoUsage,
  RATE_LIMIT_MIN_REFETCH_MS,
  RATE_LIMIT_POLL_MS,
  unavailableRateLimits,
  type ProviderRateLimits,
} from "../../providers/model/rateLimits";

const fetches = vi.hoisted(() => ({
  claude: vi.fn(),
  codex: vi.fn(),
  opencode: vi.fn(),
  pi: vi.fn(),
}));
vi.mock("../../providers/model/rateLimitsFetch", () => ({
  fetchClaudeRateLimits: fetches.claude,
  fetchCodexRateLimits: fetches.codex,
  fetchOpencodeGoRateLimits: fetches.opencode,
}));
vi.mock("../../providers/model/piUsage", async (original) => ({
  ...(await original<typeof import("../../providers/model/piUsage")>()),
  fetchPiUsage: fetches.pi,
}));
vi.mock("../../../integrations/harness/core/availability", () => ({
  isHarnessAvailable: (id: string) => id !== "claude",
}));

const values = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (key: string) => values.get(key) ?? null,
  setItem: (key: string, value: string) => values.set(key, value),
});
const now = Date.parse("2026-10-02T12:00:00Z");
/** Create a fresh Codex caller in a synthetic project for harness/selection checks. */
const source = () => newSession("codex", "/tmp/project", "codex:test");
/** Supply an exhausted session with a past reset alongside an unexpired weekly quota. */
const claude = () =>
  parseClaudeOAuthUsage(
    JSON.stringify({
      five_hour: { utilization: 100, resets_at: now - 1000 },
      seven_day: { utilization: 25, resets_at: now + 86400000 },
    }),
  );
const host = {} as AgentAppHost; // usage.list must never call a mutating host method.
/** Exercise app dispatch with an empty host so accidental host operations fail. */
const list = (input: Record<string, unknown> = {}, session = source()) =>
  handleAgentApp(session, "usage-1", "usage.list", input, host) as Promise<
    ReturnType<typeof usageSnapshot>
  >;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(now);
  values.clear();
  clearCachedRateLimits();
  for (const fetch of Object.values(fetches)) fetch.mockReset();
  fetches.claude.mockImplementation(async () => claude());
  fetches.codex.mockImplementation(async () =>
    parseCodexRateLimits({
      primary: {
        usedPercent: 10,
        windowDurationMins: 300,
        resetsAt: now + 10000,
      },
    }),
  );
  fetches.opencode.mockImplementation(async () =>
    unavailableRateLimits("opencode", "No OpenCode Go subscription"),
  );
  fetches.pi.mockImplementation(async () =>
    unavailableRateLimits(
      "claude",
      "Sign in to this provider through Pi, then refresh usage.",
    ),
  );
});
afterEach(() => vi.useRealTimers());

describe("usage.list through the app handler", () => {
  it.each(["codex", "claude"] as const)(
    "lists all accounts independently of the %s caller without probing or changing selection",
    async (harness) => {
      saveProviderAccount({ provider: "claude", id: "work", label: "Work" });
      saveProviderAccount({
        provider: "codex",
        id: "personal",
        label: "Personal",
      });
      selectProviderAccount("claude", "/tmp/project", "work");
      const session = {
        ...source(),
        harness,
        providerAccountId: harness === "claude" ? "default" : "personal",
      };
      const before = JSON.stringify(session);
      const storage = [...values.entries()];
      setCachedRateLimits("claude", "work", claude());
      for (let repeat = 0; repeat < 3; repeat++) {
        const result = await list({}, session);
        expect(result.accounts).toEqual(
          expect.arrayContaining([
            expect.objectContaining({
              provider: "claude",
              accountId: "work",
              accountLabel: "Work",
              selectedForProject: true,
              selectedForSession: false,
              cliAvailable: false,
            }),
            expect.objectContaining({
              provider: "claude",
              accountId: "default",
              selectedForSession: harness === "claude",
              status: "unavailable",
            }),
            expect.objectContaining({
              provider: "codex",
              accountId: "personal",
              selectedForSession: harness === "codex",
            }),
            expect.objectContaining({
              provider: "cursor",
              status: "unsupported",
              windows: [],
            }),
          ]),
        );
      }
      for (const fetch of Object.values(fetches))
        expect(fetch).not.toHaveBeenCalled();
      expect(JSON.stringify(session)).toBe(before);
      expect([...values.entries()]).toEqual(storage);
      expect(providerAccounts("claude")).toHaveLength(2);
    },
  );

  it("retains expired usage and reports stale data instead of full allowance", async () => {
    setCachedRateLimits("claude", "default", claude());
    const { accounts } = await list({
      provider: "claude",
      accountId: "default",
    });
    expect(accounts).toHaveLength(1);
    expect(accounts[0]).toMatchObject({
      status: "stale",
      stale: true,
      fetchedAt: now,
      windows: [
        expect.objectContaining({
          usedPercent: 100,
          remainingPercent: 0,
          resetsAt: now - 1000,
        }),
        expect.objectContaining({ usedPercent: 25 }),
      ],
    });
    setCachedRateLimits("codex", "default", await fetches.codex());
    vi.setSystemTime(now + RATE_LIMIT_POLL_MS);
    expect((await list({ provider: "codex" })).accounts[0]).toMatchObject({
      status: "stale",
      fetchedAt: now,
    });
  });

  it("refreshes through shared deduplication, cooldown and failure backoff without dropping other accounts", async () => {
    let finish!: (limits: ProviderRateLimits) => void;
    fetches.claude.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    saveProviderAccount({ provider: "claude", id: "work", label: "Work" });
    const uiLoad = loadRateLimits("claude");
    const first = await list({ refresh: true });
    expect(
      first.accounts.find(
        (row) => row.provider === "claude" && row.accountId === "default",
      )?.status,
    ).toBe("loading");
    await list({ refresh: true });
    expect(fetches.claude.mock.calls).toEqual([["default"], ["work"]]);
    finish(claude());
    await uiLoad;
    await list({ refresh: true });
    expect(fetches.claude).toHaveBeenCalledTimes(2);
    expect(fetches.codex).toHaveBeenCalledTimes(1);
    expect(fetches.pi).toHaveBeenCalledTimes(2);
    expect(fetches.opencode).toHaveBeenCalledTimes(1);

    vi.setSystemTime(now + RATE_LIMIT_MIN_REFETCH_MS);
    fetches.claude.mockRejectedValue(new Error("429 secret-cookie=hidden"));
    await list({ provider: "claude", accountId: "default", refresh: true });
    const failed = (await list()).accounts;
    expect(
      failed.find(
        (row) => row.provider === "claude" && row.accountId === "default",
      ),
    ).toMatchObject({
      status: "fetch-error",
      stale: true,
      fetchedAt: now,
      updatedAt: now + RATE_LIMIT_MIN_REFETCH_MS,
      windows: expect.any(Array),
    });
    expect(
      failed.find((row) => row.provider === "codex")?.windows,
    ).not.toHaveLength(0);
    const count = fetches.claude.mock.calls.length;
    vi.setSystemTime(now + RATE_LIMIT_MIN_REFETCH_MS + RATE_LIMIT_POLL_MS - 1);
    await list({ provider: "claude", refresh: true });
    expect(
      fetches.claude.mock.calls.filter(([id]) => id === "default"),
    ).toHaveLength(2);
    vi.setSystemTime(now + RATE_LIMIT_MIN_REFETCH_MS + RATE_LIMIT_POLL_MS);
    await list({ provider: "claude", accountId: "default", refresh: true });
    expect(fetches.claude.mock.calls.length).toBeGreaterThan(count);
    expect(JSON.stringify(await list())).not.toContain("secret-cookie");
  });

  it("shares Pi and OpenCode snapshots and leaves unsupported rows explicit", async () => {
    const pi = await fetches.codex();
    setCachedRateLimits("pi:openai-codex", "default", pi);
    setCachedRateLimits(
      "opencode",
      "default",
      parseOpencodeGoUsage({
        usage: {
          monthly: { status: "ok", percent: 12, resetsAt: now + 10000 },
        },
      }),
    );
    const result = await list();
    expect(
      result.accounts.find(
        (row) => row.provider === "pi" && row.accountId === "openai-codex",
      )?.windows[0].usedPercent,
    ).toBe(10);
    expect(
      result.accounts.find((row) => row.provider === "opencode")?.windows[0],
    ).toMatchObject({ id: "monthly", windowMinutes: 43200, usedPercent: 12 });
    expect(getCachedRateLimits("codex").status).toBe("idle");
    expect(result.accounts.find((row) => row.provider === "omp")?.status).toBe(
      "unsupported",
    );
  });

  it("reports unavailable, authentication and fetch errors without provider messages or arbitrary fields", async () => {
    setCachedRateLimits(
      "claude",
      "default",
      errorRateLimits("claude", "401 secret-token"),
    );
    setCachedRateLimits(
      "codex",
      "default",
      errorRateLimits("codex", "transport body secret-token"),
    );
    let result = await list();
    expect(
      result.accounts.find((row) => row.provider === "claude")?.status,
    ).toBe("authentication-error");
    expect(
      result.accounts.find((row) => row.provider === "codex")?.status,
    ).toBe("fetch-error");
    expect(JSON.stringify(result)).not.toContain("secret-token");
    const parsed = parseCodexRateLimits({
      rateLimits: {
        primary: { usedPercent: 20 },
        credits: {
          hasCredits: true,
          unlimited: false,
          balance: "1.25",
          accessToken: "secret-token",
        },
      },
      rateLimitResetCredits: {
        availableCount: 1,
        credits: [
          {
            id: "secret-token",
            title: "secret-token",
            description: "secret-token",
            status: "available",
          },
        ],
      },
    });
    setCachedRateLimits("codex", "default", {
      ...parsed,
      error: null,
      secret: "secret-token",
    } as ProviderRateLimits);
    result = await list({ provider: "codex" });
    expect(result.accounts[0].credits).toEqual([
      {
        scope: "account",
        hasCredits: true,
        unlimited: false,
        balance: 1.25,
        unit: "credits",
      },
    ]);
    expect(result.accounts[0].resetCredits?.availableCount).toBe(1);
    expect(JSON.stringify(result)).not.toContain("secret-token");
  });

  it("retains an explicitly removed session account without probing it", async () => {
    const result = await list(
      { refresh: true },
      { ...source(), providerAccountId: "removed" },
    );
    expect(
      result.accounts.find((row) => row.accountId === "removed"),
    ).toMatchObject({
      status: "unavailable",
      selectedForSession: true,
      windows: [],
    });
    expect(fetches.codex).toHaveBeenCalledExactlyOnceWith("default");
  });

  it("keeps an empty successful response unavailable and validates filters before fetching", async () => {
    setCachedRateLimits("claude", "default", {
      ...idleRateLimits("claude"),
      status: "ok",
      updatedAt: now,
    });
    expect((await list({ provider: "claude" })).accounts[0]).toMatchObject({
      status: "unavailable",
      windows: [],
    });
    for (const input of [
      { refresh: "true" },
      { provider: "unknown", refresh: true },
      { accountId: "../token" },
      { credentials: true },
    ]) {
      await expect(list(input)).rejects.toThrow();
    }
    for (const fetch of Object.values(fetches))
      expect(fetch).not.toHaveBeenCalled();
  });
});

it.each(["empty", "sparse", "mixed"])(
  "keeps Codex account quota with %s normalized windows",
  async (shape) => {
    const limits = parseCodexRateLimits({
      rateLimits: {
        limitId: "codex",
        primary: {
          usedPercent: 12,
          windowDurationMins: 120,
          resetsAt: now + 10000,
        },
        secondary: {
          usedPercent: 24,
          windowDurationMins: 10080,
          resetsAt: now + 20000,
        },
      },
      rateLimitsByLimitId: {
        codex: {
          credits: { hasCredits: true, unlimited: false, balance: "1.25" },
        },
        ...(shape === "mixed"
          ? {
              "gpt-model": {
                primary: { usedPercent: 33, windowDurationMins: 60 },
              },
            }
          : {}),
      },
    });
    // Older cache writers may supply an empty normalized list beside legacy slots.
    if (shape === "empty") limits.windows = [];
    setCachedRateLimits("codex", "default", limits);
    const row = (await list({ provider: "codex" })).accounts[0];
    const scope = shape === "empty" ? "account" : "codex";
    expect(row.status).toBe("ok");
    expect(row.windows).toEqual([
      expect.objectContaining({
        scope,
        usedPercent: 12,
        remainingPercent: 88,
        windowMinutes: 120,
        resetsAt: now + 10000,
      }),
      expect.objectContaining({
        scope,
        usedPercent: 24,
        remainingPercent: 76,
        windowMinutes: 10080,
        resetsAt: now + 20000,
      }),
      ...(shape === "mixed"
        ? [
            expect.objectContaining({
              scope: "gpt-model",
              usedPercent: 33,
              windowMinutes: 60,
            }),
          ]
        : []),
    ]);
    expect(row.credits).toHaveLength(1);
    expect(fetches.codex).not.toHaveBeenCalled();
  },
);
