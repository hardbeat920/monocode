import { afterEach, describe, expect, it, vi } from "vitest";
import type { ProviderAccount } from "./providerAccounts";
import {
  createUsagePricing,
  lastProviderUsage,
  loadProviderUsage,
  resetProviderUsageCache,
  rowCost,
  summarizeUsage,
  usageBreakdown,
  usageRate,
  type AccountUsage,
  type ModelPrice,
  type UsageReport,
  type UsageRow,
} from "./providerUsage";

const personal: ProviderAccount = {
  id: "default",
  provider: "claude",
  label: "Personal",
  isDefault: true,
};
const work: ProviderAccount = {
  id: "account-work",
  provider: "claude",
  label: "Work",
};
const codex: ProviderAccount = {
  id: "default",
  provider: "codex",
  label: "Codex",
  isDefault: true,
};

const now = new Date(2026, 8, 29, 15, 0);

function row(at: Date, overrides: Partial<UsageRow> = {}): UsageRow {
  return {
    slot: Math.floor(at.getTime() / 1000),
    model: "claude-opus-5-5",
    project: "/work/monocode",
    input: 0,
    cacheRead: 0,
    cacheWrite5m: 0,
    cacheWrite1h: 0,
    output: 0,
    ...overrides,
  };
}

describe("usage rates", () => {
  it("matches dated and suffixed model ids by prefix", () => {
    expect(usageRate("claude-sonnet-4-5-20250929")?.input).toBe(3);
    expect(usageRate("claude-opus-5-5")?.input).toBe(4);
    expect(usageRate("claude-opus-5")?.input).toBe(5);
    expect(usageRate("claude-opus-4-1-20250805")?.input).toBe(15);
    expect(usageRate("gpt-5.5-codex")?.input).toBe(5);
    expect(usageRate("gpt-5-codex")?.input).toBe(1.25);
  });

  it("does not guess a price for an unknown model", () => {
    expect(usageRate("gpt-5.9")).toBeNull();
    expect(usageRate("some-local-model")).toBeNull();
    expect(
      rowCost(row(now, { model: "mystery", output: 1_000_000 })),
    ).toBeNull();
  });

  it("prices each kind of token at its own rate", () => {
    const cost = rowCost(
      row(now, {
        input: 1_000_000,
        cacheRead: 1_000_000,
        cacheWrite5m: 1_000_000,
        cacheWrite1h: 1_000_000,
        output: 1_000_000,
      }),
    );
    // Opus 5.5: $4 in, $0.20 cache read, $5 and $8 cache writes, $20 out.
    expect(cost).toBeCloseTo(4 + 0.2 + 5 + 8 + 20);
  });
});

describe("usage summary", () => {
  const usage: AccountUsage[] = [
    {
      account: personal,
      rows: [
        row(new Date(2026, 8, 29, 9), {
          input: 100_000,
          cacheRead: 900_000,
          output: 10_000,
        }),
        row(new Date(2026, 8, 27, 23, 45), { input: 50_000, output: 5_000 }),
        // Before the 7-day window.
        row(new Date(2026, 8, 20, 12), { input: 1_000_000 }),
      ],
    },
    {
      account: work,
      rows: [
        row(new Date(2026, 8, 29, 1), {
          model: "claude-sonnet-5",
          project: "/work/site",
          output: 20_000,
        }),
      ],
    },
    {
      account: codex,
      rows: [
        row(new Date(2026, 8, 28, 12), {
          model: "home-brew",
          input: 1_000,
          output: 1_000,
        }),
      ],
    },
  ];

  it("totals tokens and cost into local days", () => {
    const summary = summarizeUsage(usage, 7, now);
    expect(summary.days).toHaveLength(7);
    expect(summary.days[6].date).toEqual(new Date(2026, 8, 29));
    expect(summary.days.map((day) => day.tokens)).toEqual([
      0, 0, 0, 0, 55_000, 2_000, 1_030_000,
    ]);
    expect(summary.activeDays).toBe(3);
    expect(summary.tokens).toBe(1_087_000);
    expect(summary.cacheHitRate).toBeCloseTo(900_000 / 1_051_000);
    // 900K cache reads at $4 - $0.20.
    expect(summary.cacheSavings).toBeCloseTo(3.42);
    expect(summary.unpricedModels).toEqual(["home-brew"]);
  });

  it("breaks usage down by model, project and account", () => {
    const label = (account: ProviderAccount) => account.label;
    expect(
      usageBreakdown(usage, 7, now, "model", label).map((entry) => entry.key),
    ).toEqual(["claude-opus-5-5", "claude-sonnet-5", "home-brew"]);
    expect(
      usageBreakdown(usage, 7, now, "project", label).map((entry) => [
        entry.label,
        entry.title,
      ]),
    ).toEqual([
      ["monocode", "/work/monocode"],
      ["site", "/work/site"],
    ]);
    const accounts = usageBreakdown(usage, 7, now, "account", label);
    expect(accounts.map((entry) => entry.label)).toEqual([
      "Personal",
      "Work",
      "Codex",
    ]);
    expect(accounts[0].share).toBe(1);
  });
});

describe("OpenRouter pricing", () => {
  const perToken = (perMillion: number) => perMillion / 1_000_000;
  const pricing = createUsagePricing([
    {
      id: "anthropic/claude-opus-5.5",
      input: perToken(4.4),
      output: perToken(22),
      cacheRead: perToken(0.22),
      cacheWrite: perToken(5.5),
    },
    {
      id: "anthropic/claude-sonnet-4",
      input: perToken(3.3),
      output: perToken(16),
    },
    { id: "openai/gpt-5.5", input: perToken(5.5), output: perToken(33) },
    { id: "openai/gpt-5", input: perToken(1.5), output: perToken(12) },
  ]);

  it("matches dashed Claude ids to OpenRouter's dotted ids", () => {
    const rate = pricing("claude-opus-5-5");
    expect(rate?.input).toBeCloseTo(4.4);
    expect(rate?.cacheRead).toBeCloseTo(0.22);
    expect(rate?.cacheWrite5m).toBeCloseTo(5.5);
    // No 1-hour write price listed: twice the input rate.
    expect(rate?.cacheWrite1h).toBeCloseTo(8.8);
  });

  it("keeps OpenAI dots, so gpt-5.5 never falls back to gpt-5", () => {
    expect(pricing("gpt-5.5-codex")?.input).toBeCloseTo(5.5);
    expect(pricing("gpt-5-codex")?.input).toBeCloseTo(1.5);
  });

  it("uses the built-in OpenAI cache discount when OpenRouter lists none", () => {
    // Built-in gpt-5.5 caches at $0.50.
    expect(pricing("gpt-5.5-codex")?.cacheRead).toBeCloseTo(0.5);
    const unknown = createUsagePricing([
      { id: "openai/gpt-9", input: perToken(2), output: perToken(8) },
    ]);
    expect(unknown("gpt-9")?.cacheRead).toBeCloseTo(2);
  });

  it("falls back to built-in prices when OpenRouter is less specific or silent", () => {
    // OpenRouter only knows the family; the built-in table knows 4.5.
    expect(pricing("claude-sonnet-4-5-20250929")?.input).toBe(3);
    expect(pricing("claude-haiku-4-5")?.input).toBe(1);
    expect(createUsagePricing()("claude-opus-5-5")?.input).toBe(4);
    expect(pricing("mystery")).toBeNull();
  });

  it("feeds the summary", () => {
    const summary = summarizeUsage(
      [{ account: personal, rows: [row(now, { output: 1_000_000 })] }],
      7,
      now,
      pricing,
    );
    expect(summary.cost).toBeCloseTo(22);
  });
});

describe("loading usage", () => {
  afterEach(() => resetProviderUsageCache());

  const report = (rows: UsageRow[]): UsageReport => ({
    rows,
    found: true,
    filesScanned: 1,
    filesParsed: 1,
    bytesRead: 1,
  });

  it("shares a read already in flight and remembers its result", async () => {
    let finish: (value: UsageReport) => void = () => {};
    const fetch = vi.fn(
      () => new Promise<UsageReport>((resolve) => (finish = resolve)),
    );
    const first = loadProviderUsage(personal, 1_000, fetch);
    const second = loadProviderUsage(personal, 1_000, fetch);
    expect(second).toBe(first);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(lastProviderUsage(personal, 1_000)).toBeNull();

    const result = report([row(now)]);
    finish(result);
    await first;
    expect(lastProviderUsage(personal, 1_000)).toBe(result);
    // A new day's range is its own read.
    expect(lastProviderUsage(personal, 2_000)).toBeNull();
  });

  it("starts a new read once the last one finished, or for another account", async () => {
    const fetch = vi.fn(async () => report([]));
    await loadProviderUsage(personal, 1_000, fetch);
    await loadProviderUsage(personal, 1_000, fetch);
    await loadProviderUsage(work, 1_000, fetch);
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  it("does not remember a failed read", async () => {
    const fetch = vi.fn(async () => {
      throw new Error("unreadable");
    });
    await expect(loadProviderUsage(personal, 1_000, fetch)).rejects.toThrow();
    expect(lastProviderUsage(personal, 1_000)).toBeNull();
    await expect(loadProviderUsage(personal, 1_000, fetch)).rejects.toThrow();
    expect(fetch).toHaveBeenCalledTimes(2);
  });
});

describe("usage performance", () => {
  // A month of heavy use: thousands of 15-minute buckets across many models,
  // priced from a long OpenRouter list.
  const prices: ModelPrice[] = Array.from({ length: 100 }, (_, index) => ({
    id:
      index % 2
        ? `anthropic/claude-model-${index}.${index % 7}`
        : `openai/gpt-${index}.${index % 5}`,
    input: 0.000003,
    output: 0.000015,
    cacheRead: 0.0000003,
  }));
  const models = [
    // Logged as Claude Code and Codex name them.
    ...prices
      .slice(0, 20)
      .map((price) =>
        price.id.startsWith("anthropic/")
          ? price.id.slice("anthropic/".length).replace(/\./g, "-")
          : price.id.slice("openai/".length),
      ),
    "claude-opus-5-5-20260901",
    "gpt-5.5-codex",
    "unknown-model",
  ];
  const usage: AccountUsage[] = [personal, work, codex].map(
    (account, accountIndex) => ({
      account,
      rows: Array.from({ length: 1_000 }, (_, index) =>
        row(new Date(now.getTime() - (index * 3 + accountIndex) * 15 * 60_000), {
          model: models[(index + accountIndex) % models.length],
          project: `/work/project-${index % 12}`,
          input: 1_000 + index,
          cacheRead: 10_000,
          cacheWrite5m: 500,
          output: 2_000,
        }),
      ),
    }),
  );

  it("summarizes and breaks down 3,000 rows quickly", () => {
    const label = (account: ProviderAccount) => account.label;
    const measure = () => {
      const pricing = createUsagePricing(prices);
      const started = performance.now();
      const summary = summarizeUsage(usage, 30, now, pricing);
      for (const by of ["model", "project", "account"] as const) {
        usageBreakdown(usage, 30, now, by, label, pricing);
      }
      return { summary, elapsed: performance.now() - started };
    };
    const runs = Array.from({ length: 5 }, measure);
    expect(runs[0].summary.tokens).toBeGreaterThan(0);
    expect(runs[0].summary.unpricedModels).toEqual(["unknown-model"]);
    // The best run, so a garbage collection pause does not fail the test.
    // Looking up each row's price in the whole list took ~300 ms here.
    const best = Math.min(...runs.map((run) => run.elapsed));
    expect(best).toBeLessThan(40);
  });
});
