import { asRecord } from "../../../integrations/harness/providers/codex/codexProtocol";

export type RateLimitProvider =
  "claude" | "codex" | "opencode" | "devin" | "antigravity";

export type RateLimitStatus =
  "idle" | "fetching" | "ok" | "error" | "unavailable";

export type RateLimitWindow = {
  /** Percentage of the window consumed (0–100). */
  usedPercent: number;
  /** Window duration in minutes: 300 (5h) or 10080 (7d). */
  windowMinutes: number;
  /** Unix ms timestamp when the window resets, if known. */
  resetsAt: number | null;
};

/** A quota shared by a group of models (Antigravity), not a fixed window. */
export type RateLimitPool = {
  /** Stable within one snapshot; safe as a React key. */
  id: string;
  /** Every model reporting this quota, e.g. "Claude & GPT-OSS". */
  label: string;
  /** First family only, for the compact status-bar chip. */
  shortLabel: string;
  modelIds: string[];
  modelNames: string[];
  /** `windowMinutes` is 0: pools report a reset time, not a duration. */
  window: RateLimitWindow;
};

export type RateLimitResetCredit = {
  id: string;
  resetType: "codexRateLimits" | "unknown";
  status: "available" | "redeeming" | "redeemed" | "unknown";
  grantedAt: number | null;
  expiresAt: number | null;
  title: string | null;
  description: string | null;
};

export type RateLimitResetCredits = {
  availableCount: number;
  /** Optional detail rows; the backend can report only the aggregate count. */
  credits: RateLimitResetCredit[] | null;
};

export type ProviderRateLimits = {
  provider: RateLimitProvider;
  session: RateLimitWindow | null;
  weekly: RateLimitWindow | null;
  monthly: RateLimitWindow | null;
  /** Codex-only banked rate-limit reset rewards, when supplied by app-server. */
  resetCredits: RateLimitResetCredits | null;
  /** Devin-only extra (overage) usage balance in US dollars; negative once billed. */
  extraUsageBalance?: number | null;
  /** Antigravity-only per-model-group quotas. */
  pools?: RateLimitPool[];
  updatedAt: number;
  error: string | null;
  status: RateLimitStatus;
};

export const SESSION_WINDOW_MINUTES = 300;
export const DAILY_WINDOW_MINUTES = 1_440;
export const WEEKLY_WINDOW_MINUTES = 10_080;
export const MONTHLY_WINDOW_MINUTES = 43_200;
export const RATE_LIMIT_POLL_MS = 15 * 60_000;
export const RATE_LIMIT_MIN_REFETCH_MS = 5 * 60_000;

const WINDOW_DURATION_TOLERANCE_MINUTES = 1;

export function hasRateLimitData(limits: ProviderRateLimits): boolean {
  return Boolean(
    limits.session ||
      limits.weekly ||
      limits.monthly ||
      limits.resetCredits ||
      limits.pools?.length,
  );
}

export function idleRateLimits(
  provider: RateLimitProvider,
): ProviderRateLimits {
  return {
    provider,
    session: null,
    weekly: null,
    monthly: null,
    resetCredits: null,
    updatedAt: 0,
    error: null,
    status: "idle",
  };
}

export function fetchingRateLimits(
  provider: RateLimitProvider,
  previous?: ProviderRateLimits | null,
): ProviderRateLimits {
  if (previous && hasRateLimitData(previous)) {
    return { ...previous, status: "fetching" };
  }
  return {
    provider,
    session: previous?.session ?? null,
    weekly: previous?.weekly ?? null,
    monthly: previous?.monthly ?? null,
    resetCredits: previous?.resetCredits ?? null,
    updatedAt: previous?.updatedAt ?? 0,
    error: null,
    status: "fetching",
  };
}

export function unavailableRateLimits(
  provider: RateLimitProvider,
  error: string,
): ProviderRateLimits {
  return {
    provider,
    session: null,
    weekly: null,
    monthly: null,
    resetCredits: null,
    updatedAt: Date.now(),
    error,
    status: "unavailable",
  };
}

export function errorRateLimits(
  provider: RateLimitProvider,
  error: string,
  previous?: ProviderRateLimits | null,
): ProviderRateLimits {
  if (previous && hasRateLimitData(previous)) {
    return {
      ...previous,
      error,
      status: "error",
      updatedAt: Date.now(),
    };
  }
  return {
    provider,
    session: null,
    weekly: null,
    monthly: null,
    resetCredits: null,
    updatedAt: Date.now(),
    error,
    status: "error",
  };
}

export function clampUsedPercent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(100, Math.max(0, value));
}

export function formatUsagePercent(usedPercent: number): string {
  return `${Math.round(clampUsedPercent(usedPercent))}%`;
}

/**
 * Compact window-size label. 10080 minutes stays "wk" to match the
 * original status-bar copy.
 */
export function formatWindowLabel(windowMinutes: number): string {
  if (windowMinutes === WEEKLY_WINDOW_MINUTES) return "wk";
  if (windowMinutes === MONTHLY_WINDOW_MINUTES) return "mo";
  if (windowMinutes === SESSION_WINDOW_MINUTES) return "5h";
  if (windowMinutes === 60) return "1h";
  if (windowMinutes < 60) return `${windowMinutes}m`;
  if (windowMinutes % (60 * 24 * 7) === 0) {
    return `${windowMinutes / (60 * 24 * 7)}wk`;
  }
  if (windowMinutes % (60 * 24) === 0) {
    return `${windowMinutes / (60 * 24)}d`;
  }
  if (windowMinutes % 60 === 0) return `${windowMinutes / 60}h`;
  return `${windowMinutes}m`;
}

/**
 * Compact remaining duration, flooring to whole units: "47m", "3h 54m",
 * "6d 7h". Returns "now" once the window has already reset.
 */
export function formatResetDuration(ms: number): string {
  if (ms <= 0) return "now";
  const totalMins = Math.floor(ms / 60_000);
  if (totalMins < 60) return `${totalMins}m`;
  const hours = Math.floor(totalMins / 60);
  const mins = totalMins % 60;
  if (hours >= 24) {
    const days = Math.floor(hours / 24);
    const remHours = hours % 24;
    return remHours > 0 ? `${days}d ${remHours}h` : `${days}d`;
  }
  return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`;
}

export function formatResetCountdown(ms: number): string {
  const duration = formatResetDuration(ms);
  return duration === "now" ? "Resets now" : `Resets in ${duration}`;
}

/**
 * Status-bar chip label. Prefer remaining time when resetsAt is known;
 * fall back to the fixed window size otherwise.
 */
export function formatRateLimitWindowChipLabel(
  window: RateLimitWindow,
  now = Date.now(),
): string {
  if (window.resetsAt != null) {
    return formatResetDuration(window.resetsAt - now);
  }
  if (window.windowMinutes <= 0) return "";
  return formatWindowLabel(window.windowMinutes);
}

export function rateLimitWindowTooltip(
  window: RateLimitWindow,
  now = Date.now(),
  showRemaining = false,
): string {
  const pct = clampUsedPercent(window.usedPercent);
  const usage = `${formatUsagePercent(showRemaining ? 100 - pct : pct)} ${showRemaining ? "remaining" : "used"}`;
  if (window.resetsAt == null) {
    if (window.windowMinutes <= 0) return usage;
    return `${usage} · ${formatWindowLabel(window.windowMinutes)} window`;
  }
  return `${usage} · ${formatResetCountdown(window.resetsAt - now)}`;
}

/** When a used-up window resets; the later one when several are spent. */
export function exhaustedWindowResetAt(
  limits: ProviderRateLimits,
): number | null {
  let latest: number | null = null;
  for (const window of [
    limits.session,
    limits.weekly,
    limits.monthly,
    ...(limits.pools ?? []).map((pool) => pool.window),
  ]) {
    if (!window || window.usedPercent < 100 || window.resetsAt == null)
      continue;
    latest = Math.max(latest ?? 0, window.resetsAt);
  }
  return latest;
}

export function parseResetTimestamp(value: unknown): number | null {
  if (typeof value === "number") {
    return normalizeEpochMs(value);
  }
  if (typeof value !== "string" || value.trim() === "") return null;
  const numeric = Number(value);
  if (Number.isFinite(numeric) && value.trim() !== "") {
    return normalizeEpochMs(numeric);
  }
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
}

function normalizeEpochMs(value: number): number | null {
  if (!Number.isFinite(value) || value <= 0) return null;
  // 1e10 sits between seconds-epoch (<2286) and millisecond-epoch (>2001).
  return value > 10_000_000_000 ? value : value * 1000;
}

export function mapUsageWindow(
  raw: unknown,
  windowMinutes: number,
): RateLimitWindow | null {
  const rec = asRecord(raw);
  if (!rec) return null;
  const usedPercent = usedPercentFrom(rec);
  if (usedPercent == null) return null;
  return {
    usedPercent: clampUsedPercent(usedPercent),
    windowMinutes,
    resetsAt:
      parseResetTimestamp(rec.resets_at) ??
      parseResetTimestamp(rec.resetsAt) ??
      null,
  };
}

function usedPercentFrom(rec: Record<string, unknown>): number | null {
  const value =
    numberField(rec, "used_percentage") ??
    numberField(rec, "usedPercent") ??
    numberField(rec, "utilization");
  if (value == null) return null;
  return value;
}

export function parseClaudeOAuthUsage(body: string): ProviderRateLimits {
  let parsed: unknown;
  try {
    parsed = JSON.parse(body);
  } catch {
    return errorRateLimits("claude", "Claude usage response was not JSON");
  }
  const rec = asRecord(parsed);
  if (!rec) {
    return errorRateLimits("claude", "Claude usage response was empty");
  }
  return {
    provider: "claude",
    session: mapUsageWindow(rec.five_hour, SESSION_WINDOW_MINUTES),
    weekly: mapUsageWindow(rec.seven_day, WEEKLY_WINDOW_MINUTES),
    monthly: null,
    resetCredits: null,
    updatedAt: Date.now(),
    error: null,
    status: "ok",
  };
}

/**
 * Parse Cloud Code's fetchAvailableModels payload:
 * { models: { [id]: { displayName, quotaInfo: { remainingFraction, resetTime } } } }
 * Models drawing on the same quota report identical numbers, so they are
 * grouped by value. Proto JSON omits a zero `remainingFraction`, so a quota
 * with a reset time but no fraction is spent. Models without a display name
 * are internal (tab completion, routing) and are skipped.
 */
export function parseAntigravityUsage(body: string): ProviderRateLimits {
  let parsed: unknown;
  try {
    parsed = JSON.parse(body);
  } catch {
    return errorRateLimits(
      "antigravity",
      "Antigravity usage response was not JSON",
    );
  }
  const models = asRecord(asRecord(parsed)?.models);
  if (!models) {
    return errorRateLimits(
      "antigravity",
      "Antigravity usage response was unexpected",
    );
  }
  const groups = new Map<
    string,
    { ids: string[]; names: string[]; window: RateLimitWindow }
  >();
  for (const [id, raw] of Object.entries(models)) {
    const model = asRecord(raw);
    const name = model ? stringField(model, "displayName") : null;
    const quota = asRecord(model?.quotaInfo);
    if (!name || !quota) continue;
    const resetsAt = parseResetTimestamp(quota.resetTime);
    const fraction = numberField(quota, "remainingFraction");
    if (fraction == null && resetsAt == null) continue;
    const remaining = fraction ?? 0;
    // Untouched quotas all read 1 with no reset, so value alone would merge
    // unrelated ones; keep those apart by family. Partly used quotas group
    // by value, with the reset rounded to the minute against clock jitter.
    const key =
      remaining >= 1 && resetsAt == null
        ? `full|${modelFamily(name)}`
        : `${remaining.toFixed(4)}|${resetsAt == null ? "" : Math.round(resetsAt / 60_000)}`;
    let group = groups.get(key);
    if (!group) {
      group = {
        ids: [],
        names: [],
        window: {
          usedPercent: clampUsedPercent((1 - remaining) * 100),
          windowMinutes: 0,
          resetsAt,
        },
      };
      groups.set(key, group);
    }
    group.ids.push(id);
    if (!group.names.includes(name)) group.names.push(name);
  }
  const candidates = [...groups.values()].map((group) => {
    group.ids.sort();
    group.names.sort();
    const families = unique(group.names.map(modelFamily));
    // Coarsest first; a finer label only when a coarser one is shared.
    return {
      group,
      family: families[0] ?? "",
      labels: [
        families.join(" & "),
        unique(group.names.map(baseModelName)).join(", "),
        group.names.join(", "),
        group.ids.join(", "),
      ],
    };
  });
  const pools = candidates.map(({ group, family, labels }, index) => {
    const level = labels.findIndex((label, depth) =>
      candidates.every(
        (other, otherIndex) =>
          otherIndex === index || other.labels[depth] !== label,
      ),
    );
    const label = labels[level < 0 ? labels.length - 1 : level]!;
    const familyShared = candidates.some(
      (other, otherIndex) => otherIndex !== index && other.family === family,
    );
    return {
      id: group.ids[0]!,
      label,
      shortLabel: familyShared ? label : family,
      modelIds: group.ids,
      modelNames: group.names,
      window: group.window,
    };
  });
  pools.sort((a, b) => a.label.localeCompare(b.label));
  return {
    provider: "antigravity",
    session: null,
    weekly: null,
    monthly: null,
    resetCredits: null,
    pools,
    updatedAt: Date.now(),
    error: null,
    status: "ok",
  };
}

/** "Claude Opus 5.5 (High)" -> "Claude"; "GPT-OSS 120B (Medium)" -> "GPT-OSS". */
function modelFamily(name: string): string {
  return name.split(/\s+/)[0] ?? name;
}

/** "Gemini 3.1 Pro (High)" -> "Gemini 3.1 Pro". */
function baseModelName(name: string): string {
  return name.replace(/\s*\([^)]*\)\s*$/, "");
}

function unique(values: string[]): string[] {
  return [...new Set(values)];
}

/**
 * The pool serving the session's model. The ACP catalog and the quota API
 * may disagree on ids and on whether effort is part of the name, so fall
 * back from an exact id or name to the name without its effort suffix.
 */
export function activeRateLimitPool(
  limits: ProviderRateLimits,
  model: { id: string; name?: string } | null | undefined,
): RateLimitPool | null {
  const pools = limits.pools;
  if (!model || !pools) return null;
  const name = model.name;
  const base = name ? baseModelName(name) : null;
  return (
    pools.find((pool) => pool.modelIds.includes(model.id)) ??
    (name ? pools.find((pool) => pool.modelNames.includes(name)) : null) ??
    (base
      ? pools.find((pool) =>
          pool.modelNames.some((candidate) => baseModelName(candidate) === base),
        )
      : null) ??
    null
  );
}

type CodexWindowSnapshot = {
  usedPercent: number;
  windowDurationMins: number | null;
  resetsAt: unknown;
};

export function parseCodexRateLimits(result: unknown): ProviderRateLimits {
  const rec = asRecord(result);
  const wrapper = asRecord(rec?.rateLimits) ?? rec;
  const classified = classifyCodexWindows({
    primary: snapshotFrom(asRecord(wrapper?.primary)),
    secondary: snapshotFrom(asRecord(wrapper?.secondary)),
  });
  return {
    provider: "codex",
    session: mapCodexSnapshot(classified.session, SESSION_WINDOW_MINUTES),
    weekly: mapCodexSnapshot(classified.weekly, WEEKLY_WINDOW_MINUTES),
    monthly: mapCodexSnapshot(classified.monthly, MONTHLY_WINDOW_MINUTES),
    resetCredits: parseResetCredits(
      rec?.rateLimitResetCredits ?? rec?.rate_limit_reset_credits,
    ),
    updatedAt: Date.now(),
    error: null,
    status: "ok",
  };
}

/** What the `fetch_devin_usage` command reads from Devin's user status. */
export type DevinUsage = {
  name?: string | null;
  email?: string | null;
  plan?: string | null;
  dailyRemainingPercent?: number | null;
  dailyResetsAt?: number | null;
  weeklyRemainingPercent?: number | null;
  weeklyResetsAt?: number | null;
  extraUsageBalanceMicros?: number | null;
};

/** "$-1.67", the way Devin's own plan panel writes the balance. */
export function formatExtraUsageBalance(dollars: number): string {
  return `$${dollars.toFixed(2)}`;
}

/** Devin reports remaining quota; MonoCode meters show the share used. */
export function parseDevinUsage(usage: DevinUsage): ProviderRateLimits {
  const window = (
    remaining: number | null | undefined,
    resetsAt: number | null | undefined,
    windowMinutes: number,
  ): RateLimitWindow | null =>
    remaining == null || !Number.isFinite(remaining)
      ? null
      : {
          usedPercent: clampUsedPercent(100 - remaining),
          windowMinutes,
          resetsAt: parseResetTimestamp(resetsAt),
        };
  return {
    provider: "devin",
    session: window(usage.dailyRemainingPercent, usage.dailyResetsAt, DAILY_WINDOW_MINUTES),
    weekly: window(usage.weeklyRemainingPercent, usage.weeklyResetsAt, WEEKLY_WINDOW_MINUTES),
    monthly: null,
    resetCredits: null,
    extraUsageBalance:
      usage.extraUsageBalanceMicros == null || !Number.isFinite(usage.extraUsageBalanceMicros)
        ? null
        : usage.extraUsageBalanceMicros / 1_000_000,
    updatedAt: Date.now(),
    error: null,
    status: "ok",
  };
}

/**
 * Parse the official OpenCode Go usage payload:
 * { usage: { rolling: { status, percent, resetsAt },
 *            weekly: {...}, monthly: {...} } }
 * `percent` is percent used, matching the dashboard.
 */
export function parseOpencodeGoUsage(result: unknown): ProviderRateLimits {
  const rec = asRecord(result);
  const usage = asRecord(rec?.usage) ?? rec;
  return {
    provider: "opencode",
    session: mapOpencodeGoWindow(usage?.rolling, SESSION_WINDOW_MINUTES),
    weekly: mapOpencodeGoWindow(usage?.weekly, WEEKLY_WINDOW_MINUTES),
    monthly: mapOpencodeGoWindow(usage?.monthly, MONTHLY_WINDOW_MINUTES),
    resetCredits: null,
    updatedAt: Date.now(),
    error: null,
    status: "ok",
  };
}

function mapOpencodeGoWindow(
  raw: unknown,
  windowMinutes: number,
): RateLimitWindow | null {
  const rec = asRecord(raw);
  if (!rec) return null;
  // Require an explicit valid status; unknown shapes are dropped so the
  // caller can treat a fully empty payload as an error, not a snapshot.
  const status = rec.status;
  if (status !== "ok" && status !== "rate-limited") return null;
  const usedPercent =
    numberField(rec, "percent") ?? numberField(rec, "usedPercent");
  if (usedPercent == null) return null;
  return {
    usedPercent: clampUsedPercent(usedPercent),
    windowMinutes,
    resetsAt:
      parseResetTimestamp(rec.resetsAt) ?? parseResetTimestamp(rec.resets_at),
  };
}

function parseResetCredits(raw: unknown): RateLimitResetCredits | null {
  const rec = asRecord(raw);
  if (!rec) return null;
  const count =
    numberField(rec, "availableCount") ?? numberField(rec, "available_count");
  if (count == null) return null;
  const rawCredits = rec.credits;
  const credits = Array.isArray(rawCredits)
    ? rawCredits
        .map(parseResetCredit)
        .filter((credit): credit is RateLimitResetCredit => credit != null)
    : null;
  return {
    availableCount: Math.max(0, Math.floor(count)),
    credits,
  };
}

function parseResetCredit(raw: unknown): RateLimitResetCredit | null {
  const rec = asRecord(raw);
  if (!rec || typeof rec.id !== "string" || rec.id.trim() === "") {
    return null;
  }
  const resetType =
    rec.resetType === "codexRateLimits" ? "codexRateLimits" : "unknown";
  const status =
    rec.status === "available" ||
    rec.status === "redeeming" ||
    rec.status === "redeemed"
      ? rec.status
      : "unknown";
  return {
    id: rec.id,
    resetType,
    status,
    grantedAt: parseResetTimestamp(rec.grantedAt ?? rec.granted_at),
    expiresAt: parseResetTimestamp(rec.expiresAt ?? rec.expires_at),
    title: stringField(rec, "title"),
    description: stringField(rec, "description"),
  };
}

function snapshotFrom(
  rec: Record<string, unknown> | null,
): CodexWindowSnapshot | null {
  if (!rec) return null;
  const usedPercent =
    numberField(rec, "usedPercent") ??
    numberField(rec, "used_percent") ??
    numberField(rec, "used_percentage");
  if (usedPercent == null) return null;
  return {
    usedPercent,
    windowDurationMins:
      numberField(rec, "windowDurationMins") ??
      numberField(rec, "window_duration_mins") ??
      null,
    resetsAt: rec.resetsAt ?? rec.resets_at,
  };
}

function classifyCodexWindows(input: {
  primary: CodexWindowSnapshot | null;
  secondary: CodexWindowSnapshot | null;
}): {
  session: CodexWindowSnapshot | null;
  weekly: CodexWindowSnapshot | null;
  monthly: CodexWindowSnapshot | null;
} {
  let session: CodexWindowSnapshot | null = null;
  let weekly: CodexWindowSnapshot | null = null;
  let monthly: CodexWindowSnapshot | null = null;
  for (const window of [input.primary, input.secondary]) {
    if (!window) continue;
    const kind = classifyWindowDuration(window.windowDurationMins);
    if (kind === "session" && !session) session = window;
    else if (kind === "weekly" && !weekly) weekly = window;
    else if (kind === "monthly" && !monthly) monthly = window;
  }
  if (
    !session &&
    input.primary &&
    classifyWindowDuration(input.primary.windowDurationMins) === null
  ) {
    session = input.primary;
  }
  if (
    !weekly &&
    input.secondary &&
    classifyWindowDuration(input.secondary.windowDurationMins) === null
  ) {
    weekly = input.secondary;
  }
  return { session, weekly, monthly };
}

function classifyWindowDuration(
  duration: number | null,
): "session" | "weekly" | "monthly" | null {
  if (duration == null || !Number.isFinite(duration)) return null;
  if (
    Math.abs(duration - SESSION_WINDOW_MINUTES) <=
    WINDOW_DURATION_TOLERANCE_MINUTES
  ) {
    return "session";
  }
  if (
    Math.abs(duration - WEEKLY_WINDOW_MINUTES) <=
    WINDOW_DURATION_TOLERANCE_MINUTES
  ) {
    return "weekly";
  }
  // Free plans get a single 30-day window.
  if (
    Math.abs(duration - MONTHLY_WINDOW_MINUTES) <=
    WINDOW_DURATION_TOLERANCE_MINUTES
  ) {
    return "monthly";
  }
  return null;
}

function mapCodexSnapshot(
  raw: CodexWindowSnapshot | null,
  windowMinutes: number,
): RateLimitWindow | null {
  if (!raw) return null;
  return {
    usedPercent: clampUsedPercent(raw.usedPercent),
    windowMinutes,
    resetsAt: parseResetTimestamp(raw.resetsAt),
  };
}

function numberField(rec: Record<string, unknown>, key: string): number | null {
  const value = rec[key];
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() !== "") {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return null;
}

function stringField(rec: Record<string, unknown>, key: string): string | null {
  const value = rec[key];
  return typeof value === "string" && value.trim() !== "" ? value.trim() : null;
}
