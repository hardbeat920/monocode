import type {
  ProviderRateLimits,
  RateLimitWindow,
  ScopedRateLimitWindow,
} from "./rateLimits";
import type { Block, Session } from "./session";
import type { UsageTokens } from "./sessionUsage";
import { isStandaloneCommand, type BuiltinSkill } from "./skills";

export const USAGE_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "usage",
  invocation: "usage",
  description: "Show session totals and Claude rate-limit windows.",
  scope: "builtin",
  source: "monocode",
};

export const USAGE_SNAPSHOT_MAX_AGE_MS = 30_000;

export function isUsageCommand(text: string): boolean {
  return isStandaloneCommand(text, USAGE_COMMAND.name);
}

export type UsageReportLimits = Pick<
  ProviderRateLimits,
  "session" | "weekly" | "weeklyByModel" | "error"
>;

export type UsageReportInput = {
  session: Pick<Session, "blocks" | "usage">;
  limits: UsageReportLimits;
};

export type UsageReportOptions = {
  now: number;
  timeZone: string;
};

const BAR_CELLS = 50;
const NO_TOKENS: UsageTokens = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
};

export function turnTimeMs(blocks: readonly Block[], now: number): number {
  let total = 0;
  for (const block of blocks) {
    if (block.role !== "user") continue;
    if (block.durationMs != null) total += block.durationMs;
    else if (block.startedAt != null)
      total += Math.max(0, now - block.startedAt);
  }
  return total;
}

/** 50-cell bar: one `█` per 2%, `▌` for an odd percent, then "N% used". */
export function usageBar(usedPercent: number): string {
  const percent = Math.min(
    100,
    Math.max(0, Math.round(Number.isFinite(usedPercent) ? usedPercent : 0)),
  );
  const full = Math.floor(percent / 2);
  const half = percent % 2 === 1;
  const bar = "█".repeat(full) + (half ? "▌" : "");
  return `${bar.padEnd(BAR_CELLS)} ${percent}% used`;
}

export function formatResetLine(
  resetsAt: number | null,
  options: UsageReportOptions,
): string {
  if (resetsAt == null) return "Resets at an unknown time";
  const { now, timeZone } = options;
  const time = clockTime(resetsAt, timeZone);
  const zone = ` (${timeZone})`;
  if (dayKey(resetsAt, timeZone) === dayKey(now, timeZone)) {
    return `Resets ${time}${zone}`;
  }
  const date = new Intl.DateTimeFormat("en-US", {
    timeZone,
    month: "short",
    day: "numeric",
  }).format(resetsAt);
  return `Resets ${date} at ${time}${zone}`;
}

export function formatSessionDuration(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) return `${hours}h ${minutes}m ${seconds}s`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}

export function formatUsageReport(
  input: UsageReportInput,
  options: UsageReportOptions,
): string {
  const sections: string[][] = [
    [`/${USAGE_COMMAND.name}`],
    sessionSection(input.session, options.now),
  ];
  const { limits } = input;
  if (limits.session) {
    sections.push(windowSection("Current session", limits.session, options));
  }
  if (limits.weekly) {
    sections.push(
      windowSection("Current week (all models)", limits.weekly, options),
    );
  }
  for (const scoped of limits.weeklyByModel ?? []) {
    sections.push(scopedWindowSection(scoped, options));
  }
  if (!limits.session && !limits.weekly && !limits.weeklyByModel?.length) {
    sections.push([`Rate limits unavailable: ${limits.error ?? "no data"}`]);
  }
  return sections.map((lines) => lines.join("\n")).join("\n\n");
}

function sessionSection(
  session: Pick<Session, "blocks" | "usage">,
  now: number,
): string[] {
  const usage = session.usage;
  const tokens = usage?.tokens ?? NO_TOKENS;
  return [
    "Session",
    sessionLine("Total cost:", `$${(usage?.costUsd ?? 0).toFixed(4)}`),
    sessionLine(
      "Total duration (API):",
      formatSessionDuration(usage?.apiMs ?? 0),
    ),
    sessionLine(
      "Total duration (wall):",
      formatSessionDuration(turnTimeMs(session.blocks, now)),
    ),
    sessionLine(
      "Usage:",
      [
        `${formatCount(tokens.input)} input`,
        `${formatCount(tokens.output)} output`,
        `${formatCount(tokens.cacheRead)} cache read`,
        `${formatCount(tokens.cacheWrite)} cache write`,
      ].join(", "),
    ),
  ];
}

const SESSION_LABEL_WIDTH = "Total duration (wall):".length + 1;

function sessionLine(label: string, value: string): string {
  return `${label.padEnd(SESSION_LABEL_WIDTH)}${value}`;
}

function windowSection(
  title: string,
  window: RateLimitWindow,
  options: UsageReportOptions,
): string[] {
  return [
    title,
    usageBar(window.usedPercent),
    formatResetLine(window.resetsAt, options),
  ];
}

function scopedWindowSection(
  scoped: ScopedRateLimitWindow,
  options: UsageReportOptions,
): string[] {
  return windowSection(
    `Current week (${scoped.label})`,
    scoped.window,
    options,
  );
}

function formatCount(value: number): string {
  return Math.round(value).toLocaleString("en-US");
}

function clockTime(at: number, timeZone: string): string {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone,
    hour: "numeric",
    minute: "2-digit",
    hour12: true,
  }).formatToParts(at);
  const part = (type: string) =>
    parts.find((p) => p.type === type)?.value ?? "";
  const hour = part("hour");
  const minute = part("minute");
  const period = part("dayPeriod").toLowerCase();
  return minute === "00" ? `${hour}${period}` : `${hour}:${minute}${period}`;
}

function dayKey(at: number, timeZone: string): string {
  return new Intl.DateTimeFormat("en-CA", {
    timeZone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(at);
}
