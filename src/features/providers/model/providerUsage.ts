import { invoke } from "@tauri-apps/api/core";
import type {
  ProviderAccount,
  ProviderAccountProvider,
} from "./providerAccounts";

/** One 15-minute bucket of token counts for a model and working directory. */
export type UsageRow = {
  /** Bucket start, Unix seconds. */
  slot: number;
  model: string;
  /** Working directory the session ran in; empty when unknown. */
  project: string;
  /** Uncached input tokens. */
  input: number;
  cacheRead: number;
  cacheWrite5m: number;
  cacheWrite1h: number;
  output: number;
};

export type UsageReport = {
  rows: UsageRow[];
  /** False when the account has never written a session log. */
  found: boolean;
  filesScanned: number;
  /** Logs read this time; unchanged ones are reused from the last scan. */
  filesParsed: number;
  bytesRead: number;
};

/** Longest range the Usage section offers; one scan covers every range. */
export const USAGE_MAX_DAYS = 30;

export async function fetchProviderUsage(
  provider: ProviderAccountProvider,
  accountId: string,
  sinceMs: number,
): Promise<UsageReport> {
  return invoke<UsageReport>("provider_usage_report", {
    provider,
    accountId,
    sinceMs,
  });
}

type UsageRequest = {
  sinceMs: number;
  promise: Promise<UsageReport>;
};

const pendingUsage = new Map<string, UsageRequest>();
const lastUsage = new Map<string, { sinceMs: number; report: UsageReport }>();

function usageKey(account: ProviderAccount): string {
  return `${account.provider}:${account.id}`;
}

/**
 * Reads an account's usage, sharing a read already in flight for the same
 * range, so reopening Settings mid-scan does not start a second one.
 */
export function loadProviderUsage(
  account: ProviderAccount,
  sinceMs: number,
  fetch: typeof fetchProviderUsage = fetchProviderUsage,
): Promise<UsageReport> {
  const key = usageKey(account);
  const pending = pendingUsage.get(key);
  if (pending?.sinceMs === sinceMs) return pending.promise;
  const promise = fetch(account.provider, account.id, sinceMs)
    .then((report) => {
      lastUsage.set(key, { sinceMs, report });
      return report;
    })
    .finally(() => {
      if (pendingUsage.get(key)?.promise === promise) pendingUsage.delete(key);
    });
  pendingUsage.set(key, { sinceMs, promise });
  return promise;
}

/** The last usage read for an account, to show while it is read again. */
export function lastProviderUsage(
  account: ProviderAccount,
  sinceMs: number,
): UsageReport | null {
  const last = lastUsage.get(usageKey(account));
  return last && last.sinceMs === sinceMs ? last.report : null;
}

/** Test hook: forgets every read. */
export function resetProviderUsageCache(): void {
  pendingUsage.clear();
  lastUsage.clear();
}

/** US dollars per million tokens. */
type Rate = {
  input: number;
  output: number;
  cacheRead: number;
  /** Anthropic bills cache writes; OpenAI does not. */
  cacheWrite5m?: number;
  cacheWrite1h?: number;
};

const anthropic = (input: number, output: number, cacheRead: number): Rate => ({
  input,
  output,
  cacheRead,
  cacheWrite5m: input * 1.25,
  cacheWrite1h: input * 2,
});

const openai = (input: number, output: number, cacheRead: number): Rate => ({
  input,
  output,
  cacheRead,
});

/**
 * Standard pay-as-you-go API rates, matched by model id prefix. Update this
 * table when providers change prices or release models; unknown models are
 * counted in tokens but left out of cost.
 */
const RATES: Record<string, Rate> = {
  "claude-fable-5-1": anthropic(10, 50, 0.25),
  "claude-mythos-5-1": anthropic(10, 50, 0.25),
  "claude-fable-5": anthropic(10, 50, 1),
  "claude-mythos-5": anthropic(10, 50, 1),
  "claude-opus-5-5": anthropic(4, 20, 0.2),
  "claude-opus-5": anthropic(5, 25, 0.5),
  "claude-opus-4-8": anthropic(5, 25, 0.5),
  "claude-opus-4-7": anthropic(5, 25, 0.5),
  "claude-opus-4-6": anthropic(5, 25, 0.5),
  "claude-opus-4-5": anthropic(5, 25, 0.5),
  "claude-opus-4-1": anthropic(15, 75, 1.5),
  "claude-opus-4": anthropic(15, 75, 1.5),
  "claude-sonnet-5-5": anthropic(2, 10, 0.2),
  "claude-sonnet-5": anthropic(2, 10, 0.2),
  "claude-sonnet-4-6": anthropic(3, 15, 0.3),
  "claude-sonnet-4-5": anthropic(3, 15, 0.3),
  "claude-sonnet-4": anthropic(3, 15, 0.3),
  "claude-3-7-sonnet": anthropic(3, 15, 0.3),
  "claude-haiku-4-5": anthropic(1, 5, 0.1),
  "claude-3-5-haiku": anthropic(0.8, 4, 0.08),
  "gpt-5.6-sol": openai(5, 30, 0.5),
  "gpt-5.6-terra": openai(2, 12, 0.2),
  "gpt-5.6-luna": openai(0.2, 1.2, 0.02),
  "gpt-5.5-pro": openai(30, 180, 30),
  "gpt-5.5": openai(5, 30, 0.5),
  "gpt-5.4-pro": openai(30, 180, 30),
  "gpt-5.4-mini": openai(0.75, 4.5, 0.075),
  "gpt-5.4-nano": openai(0.2, 1.25, 0.02),
  "gpt-5.4": openai(2.5, 15, 0.25),
  "gpt-5.3": openai(1.75, 14, 0.175),
  "gpt-5.2": openai(1.75, 14, 0.175),
  "gpt-5.1-codex-mini": openai(0.25, 2, 0.025),
  "gpt-5.1": openai(1.25, 10, 0.125),
  "gpt-5-codex-mini": openai(0.25, 2, 0.025),
  "gpt-5-mini": openai(0.25, 2, 0.025),
  "gpt-5-nano": openai(0.05, 0.4, 0.005),
  "gpt-5": openai(1.25, 10, 0.125),
};

function normalizeModelId(model: string): string {
  return model
    .trim()
    .toLowerCase()
    .replace(/^(anthropic|openai)[/.]/, "");
}

/** Length of `key` when it names `id` or a dated/suffixed variant of it. */
function prefixMatch(id: string, key: string): number {
  return id === key || (id.startsWith(key) && /[-@]/.test(id[key.length]))
    ? key.length
    : -1;
}

function bestRate(
  id: string,
  table: Record<string, Rate>,
): { rate: Rate; length: number } | null {
  let best: { rate: Rate; length: number } | null = null;
  for (const [key, rate] of Object.entries(table)) {
    const length = prefixMatch(id, key);
    if (length > (best?.length ?? -1)) best = { rate, length };
  }
  return best;
}

/** Remembers each model's rate, since a report repeats a few models many times. */
function memoizeByModel(lookup: UsagePricing): UsagePricing {
  const rates = new Map<string, Rate | null>();
  return (model) => {
    let rate = rates.get(model);
    if (rate === undefined) {
      rate = lookup(model);
      rates.set(model, rate);
    }
    return rate;
  };
}

/** Built-in rate for a model id such as `claude-sonnet-4-5-20250929` or `gpt-5.5-codex`. */
export const usageRate: UsagePricing = memoizeByModel(
  (model) => bestRate(normalizeModelId(model), RATES)?.rate ?? null,
);

/** Looks up the rate for a model id as it appears in the session logs. */
export type UsagePricing = (model: string) => Rate | null;

/** One model's list price in dollars per token, from OpenRouter. */
export type ModelPrice = {
  id: string;
  input: number;
  output: number;
  cacheRead?: number | null;
  cacheWrite?: number | null;
  cacheWrite1h?: number | null;
};

export type ModelPrices = {
  models: ModelPrice[];
  /** Unix seconds. */
  fetchedAt: number;
};

/** OpenRouter's public price list, cached by the app for a day. */
export async function fetchModelPrices(): Promise<ModelPrices | null> {
  try {
    return await invoke<ModelPrices>("provider_model_prices");
  } catch {
    return null;
  }
}

/**
 * Prices from OpenRouter where it lists the model, the built-in table
 * otherwise. The more specific match wins, so a dated Claude id is not
 * priced as its family when only the built-in table knows the exact model.
 */
export function createUsagePricing(models: ModelPrice[] = []): UsagePricing {
  const remote: Record<string, Rate> = {};
  for (const model of models) {
    const perMillion = (value: number) => value * 1_000_000;
    const input = perMillion(model.input);
    if (model.id.startsWith("anthropic/")) {
      // Claude Code logs dashed ids (claude-opus-5-5); OpenRouter uses dots.
      remote[normalizeModelId(model.id).replace(/\./g, "-")] = {
        input,
        output: perMillion(model.output),
        cacheRead:
          model.cacheRead != null ? perMillion(model.cacheRead) : input * 0.1,
        cacheWrite5m:
          model.cacheWrite != null
            ? perMillion(model.cacheWrite)
            : input * 1.25,
        cacheWrite1h:
          model.cacheWrite1h != null
            ? perMillion(model.cacheWrite1h)
            : input * 2,
      };
    } else if (model.id.startsWith("openai/")) {
      const id = normalizeModelId(model.id);
      remote[id] = {
        input,
        output: perMillion(model.output),
        // Without a listed cache price, keep the built-in discount for this
        // model rather than billing cache reads as full input.
        cacheRead:
          model.cacheRead != null
            ? perMillion(model.cacheRead)
            : (usageRate(id)?.cacheRead ?? input),
      };
    }
  }
  return memoizeByModel((model) => {
    const id = normalizeModelId(model);
    const fromRemote = bestRate(id, remote);
    const builtIn = bestRate(id, RATES);
    if (!fromRemote) return builtIn?.rate ?? null;
    if (!builtIn) return fromRemote.rate;
    return fromRemote.length >= builtIn.length ? fromRemote.rate : builtIn.rate;
  });
}

export function rowTokens(row: UsageRow): number {
  return rowInput(row) + row.output;
}

/** Every input token, cached or not. */
function rowInput(row: UsageRow): number {
  return row.input + row.cacheRead + row.cacheWrite5m + row.cacheWrite1h;
}

/** Estimated cost in dollars, or null when the model has no known rate. */
export function rowCost(
  row: UsageRow,
  pricing: UsagePricing = usageRate,
): number | null {
  const rate = pricing(row.model);
  if (!rate) return null;
  return (
    (row.input * rate.input +
      row.cacheRead * rate.cacheRead +
      row.cacheWrite5m * (rate.cacheWrite5m ?? rate.input) +
      row.cacheWrite1h * (rate.cacheWrite1h ?? rate.input) +
      row.output * rate.output) /
    1_000_000
  );
}

/** What the cache reads would have cost as ordinary input. */
function rowCacheSavings(row: UsageRow, pricing: UsagePricing): number {
  const rate = pricing(row.model);
  if (!rate) return 0;
  return (row.cacheRead * (rate.input - rate.cacheRead)) / 1_000_000;
}

export type AccountUsage = {
  account: ProviderAccount;
  rows: UsageRow[];
};

export type UsageDay = {
  /** Local midnight. */
  date: Date;
  tokens: number;
  cost: number;
};

export type UsageBreakdown = "model" | "project" | "account";

export type UsageBreakdownRow = {
  key: string;
  label: string;
  /** Full value behind a shortened label, such as a project's path. */
  title?: string;
  provider: ProviderAccountProvider | null;
  tokens: number;
  cost: number;
  /** Share of the largest row, for the bar. */
  share: number;
};

export type UsageSummary = {
  days: UsageDay[];
  tokens: number;
  cost: number;
  activeDays: number;
  /** Cache reads as a share of all input tokens. */
  cacheHitRate: number;
  cacheSavings: number;
  /** Models seen in the logs that have no rate, so their cost is missing. */
  unpricedModels: string[];
};

/** The last `dayCount` local calendar days, oldest first, ending today. */
export function usageDays(dayCount: number, now: Date): Date[] {
  return Array.from({ length: dayCount }, (_, index) => {
    const date = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    date.setDate(date.getDate() - (dayCount - 1 - index));
    return date;
  });
}

/**
 * Rows that fall inside the last `dayCount` local days, with the index of
 * the day each falls on.
 */
function rowsInRange(
  usage: AccountUsage[],
  dayCount: number,
  now: Date,
): { entry: AccountUsage; row: UsageRow; day: number }[] {
  const dates = usageDays(dayCount, now);
  const last = dates[dates.length - 1];
  // Local midnights in Unix seconds; a day can be 23 or 25 hours long.
  const bounds = [
    ...dates,
    new Date(last.getFullYear(), last.getMonth(), last.getDate() + 1),
  ].map((date) => date.getTime() / 1000);
  const end = bounds[bounds.length - 1];
  const result: { entry: AccountUsage; row: UsageRow; day: number }[] = [];
  for (const entry of usage) {
    for (const row of entry.rows) {
      if (row.slot < bounds[0] || row.slot >= end) continue;
      let day = 0;
      while (row.slot >= bounds[day + 1]) day += 1;
      result.push({ entry, row, day });
    }
  }
  return result;
}

export function summarizeUsage(
  usage: AccountUsage[],
  dayCount: number,
  now: Date,
  pricing: UsagePricing = usageRate,
): UsageSummary {
  const days = usageDays(dayCount, now).map((date) => ({
    date,
    tokens: 0,
    cost: 0,
  }));
  let input = 0;
  let cacheRead = 0;
  let cacheSavings = 0;
  const unpriced = new Set<string>();

  for (const { row, day } of rowsInRange(usage, dayCount, now)) {
    const target = days[day];
    const cost = rowCost(row, pricing);
    if (cost == null) unpriced.add(row.model);
    target.tokens += rowTokens(row);
    target.cost += cost ?? 0;
    input += rowInput(row);
    cacheRead += row.cacheRead;
    cacheSavings += rowCacheSavings(row, pricing);
  }

  return {
    days,
    tokens: days.reduce((sum, day) => sum + day.tokens, 0),
    cost: days.reduce((sum, day) => sum + day.cost, 0),
    activeDays: days.filter((day) => day.tokens > 0).length,
    cacheHitRate: input > 0 ? cacheRead / input : 0,
    cacheSavings,
    unpricedModels: [...unpriced].sort(),
  };
}

function projectName(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function usageBreakdown(
  usage: AccountUsage[],
  dayCount: number,
  now: Date,
  by: UsageBreakdown,
  accountLabel: (account: ProviderAccount) => string,
  pricing: UsagePricing = usageRate,
): UsageBreakdownRow[] {
  const rows = new Map<string, Omit<UsageBreakdownRow, "share">>();
  for (const { entry, row } of rowsInRange(usage, dayCount, now)) {
    const { account } = entry;
    const [key, label, title, provider] =
      by === "model"
        ? // The same model used from two accounts is one row.
          [row.model, row.model, undefined, account.provider]
        : by === "account"
          ? [
              `${account.provider}:${account.id}`,
              accountLabel(account),
              undefined,
              account.provider,
            ]
          : [
              row.project,
              row.project ? projectName(row.project) : "Unknown folder",
              row.project || undefined,
              null,
            ];
    const target = rows.get(key) ?? {
      key,
      label,
      title,
      provider,
      tokens: 0,
      cost: 0,
    };
    target.tokens += rowTokens(row);
    target.cost += rowCost(row, pricing) ?? 0;
    rows.set(key, target);
  }
  const sorted = [...rows.values()].sort(
    (a, b) => b.cost - a.cost || b.tokens - a.tokens,
  );
  const top = sorted[0];
  return sorted.map((row) => ({
    ...row,
    share: top
      ? top.cost > 0
        ? row.cost / top.cost
        : top.tokens > 0
          ? row.tokens / top.tokens
          : 0
      : 0,
  }));
}

export function formatUsageTokens(value: number): string {
  if (value >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(1)}B`;
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(Math.round(value));
}

export function formatUsageCost(value: number): string {
  return `$${value.toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })}`;
}
