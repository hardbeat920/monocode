import { asRecord } from "../../../integrations/harness/providers/codex/codexProtocol";

export type RateLimitProvider = "claude" | "codex" | "opencode";

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

/** Complete provider windows; the three legacy slots remain the footer view. */
export type QuotaWindow = {
  id: string;
  scope: string;
  usedPercent: number;
  windowMinutes: number | null;
  resetsAt: number | null;
};

export type ProviderRateLimits = {
  windows?: QuotaWindow[];
  extraUsage?: {
    enabled: boolean | null;
    monthlyLimit: number | null;
    usedCredits: number | null;
    usedPercent: number | null;
    currency: string | null;
  } | null;
  credits?: {
    scope: string;
    hasCredits: boolean | null;
    unlimited: boolean | null;
    balance: number | null;
  }[];
  /** Last successful snapshot time, distinct from a failed attempt's updatedAt. */
  fetchedAt?: number | null;
  provider: RateLimitProvider;
  session: RateLimitWindow | null;
  weekly: RateLimitWindow | null;
  monthly: RateLimitWindow | null;
  /** Codex-only banked rate-limit reset rewards, when supplied by app-server. */
  resetCredits: RateLimitResetCredits | null;
  updatedAt: number;
  error: string | null;
  status: RateLimitStatus;
};

export const SESSION_WINDOW_MINUTES = 300;
export const WEEKLY_WINDOW_MINUTES = 10_080;
export const MONTHLY_WINDOW_MINUTES = 43_200;
export const RATE_LIMIT_POLL_MS = 15 * 60_000;
export const RATE_LIMIT_MIN_REFETCH_MS = 5 * 60_000;

const WINDOW_DURATION_TOLERANCE_MINUTES = 1;

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
  if (
    previous &&
    (previous.session ||
      previous.weekly ||
      previous.monthly ||
      previous.resetCredits ||
      previous.windows?.length ||
      previous.extraUsage ||
      previous.credits?.length)
  ) {
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
  if (
    previous &&
    (previous.session ||
      previous.weekly ||
      previous.monthly ||
      previous.resetCredits ||
      previous.windows?.length ||
      previous.extraUsage ||
      previous.credits?.length)
  ) {
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
  return formatWindowLabel(window.windowMinutes);
}

export function rateLimitWindowTooltip(
  window: RateLimitWindow,
  now = Date.now(),
): string {
  const used = `${formatUsagePercent(window.usedPercent)} used`;
  if (window.resetsAt == null) {
    return `${used} · ${formatWindowLabel(window.windowMinutes)} window`;
  }
  return `${used} · ${formatResetCountdown(window.resetsAt - now)}`;
}

/** When a used-up window resets; the later one when several are spent. */
export function exhaustedWindowResetAt(
  limits: ProviderRateLimits,
): number | null {
  let latest: number | null = null;
  for (const window of [limits.session, limits.weekly, limits.monthly]) {
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
  const windows: QuotaWindow[] = [];
  for (const [id, scope, minutes] of [
    ["five_hour", "account", SESSION_WINDOW_MINUTES],
    ["seven_day", "account", WEEKLY_WINDOW_MINUTES],
    ["seven_day_opus", "opus", WEEKLY_WINDOW_MINUTES],
    ["seven_day_sonnet", "sonnet", WEEKLY_WINDOW_MINUTES],
    ["seven_day_oauth_apps", "oauth_apps", WEEKLY_WINDOW_MINUTES],
    ["seven_day_cowork", "cowork", WEEKLY_WINDOW_MINUTES],
    ["seven_day_overage_included", "overage_included", WEEKLY_WINDOW_MINUTES],
  ] as const) {
    const window = mapUsageWindow(rec[id], minutes);
    if (window) windows.push({ id, scope, ...window });
  }
  // Newer Claude responses put per-model weekly buckets in limits[]. Keep
  // only the known usage fields and the model label, never the raw scope.
  if (Array.isArray(rec.limits)) {
    for (const raw of rec.limits) {
      const limit = asRecord(raw);
      const model = asRecord(asRecord(limit?.scope)?.model);
      const scope = model && stringField(model, "display_name");
      if (limit?.kind !== "weekly_scoped" || !scope) continue;
      const window = mapUsageWindow(
        { utilization: limit.percent, resets_at: limit.resets_at },
        WEEKLY_WINDOW_MINUTES,
      );
      if (window) windows.push({ id: "weekly_scoped", scope, ...window });
    }
  }
  const extra = asRecord(rec.extra_usage);
  return {
    windows,
    extraUsage: extra
      ? {
          enabled:
            typeof extra.is_enabled === "boolean" ? extra.is_enabled : null,
          monthlyLimit: numberField(extra, "monthly_limit"),
          usedCredits: numberField(extra, "used_credits"),
          usedPercent: numberField(extra, "utilization"),
          currency:
            typeof extra.currency === "string" &&
            /^[A-Z]{3}$/.test(extra.currency)
              ? extra.currency
              : null,
        }
      : null,
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
  // app-server's single bucket is a compatibility view. Keep every named
  // bucket, including model-specific quotas and nonstandard window durations.
  const buckets = asRecord(rec?.rateLimitsByLimitId);
  const entries = buckets
    ? Object.entries(buckets).filter(([, value]) => asRecord(value))
    : [];
  const defaultScope = stringField(wrapper ?? {}, "limitId") ?? "account";
  if (wrapper && !entries.some(([id]) => id === defaultScope)) {
    entries.unshift([defaultScope, wrapper]);
  }
  const windows: QuotaWindow[] = [];
  const credits: NonNullable<ProviderRateLimits["credits"]> = [];
  for (const [scope, value] of entries) {
    const bucket = asRecord(value);
    if (!bucket) continue;
    for (const id of ["primary", "secondary"] as const) {
      // A named bucket may carry only credits. Recover missing windows from
      // the compatibility view without losing other model-scoped buckets.
      const window =
        snapshotFrom(asRecord(bucket[id])) ??
        (scope === defaultScope ? snapshotFrom(asRecord(wrapper?.[id])) : null);
      if (window)
        windows.push({
          id,
          scope,
          usedPercent: clampUsedPercent(window.usedPercent),
          windowMinutes: window.windowDurationMins,
          resetsAt: parseResetTimestamp(window.resetsAt),
        });
    }
    const credit = asRecord(bucket.credits);
    if (credit)
      credits.push({
        scope,
        hasCredits:
          typeof credit.hasCredits === "boolean" ? credit.hasCredits : null,
        unlimited:
          typeof credit.unlimited === "boolean" ? credit.unlimited : null,
        balance: numberField(credit, "balance"),
      });
  }
  return {
    windows,
    credits,
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
    windowMinutes: raw.windowDurationMins ?? windowMinutes,
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
