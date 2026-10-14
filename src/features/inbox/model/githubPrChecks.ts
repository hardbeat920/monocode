import { invoke } from "@tauri-apps/api/core";
import { getLocaleTag, t } from "../../../shared/i18n/locale";
import type { TextKey } from "../../../shared/i18n/catalog";

export type GithubPrCheckState =
  "pass" | "fail" | "pending" | "skipping" | "cancel" | "unknown";

export type GithubPrCheck = {
  name: string;
  workflow: string;
  state: GithubPrCheckState;
  url: string | null;
  startedAt: string | null;
  completedAt: string | null;
};

export type GithubPrChecks = {
  headOid: string;
  checks: GithubPrCheck[];
};

export type GithubCheckDetails = {
  steps: {
    name: string;
    state: GithubPrCheckState;
    startedAt: string | null;
    completedAt: string | null;
  }[];
  annotations: { path: string; line: number; message: string; level: string }[];
  notice: string | null;
};

export function githubActionsJobId(
  url: string | null,
  repo: string,
): string | null {
  if (!url) return null;
  try {
    const parsed = new URL(url);
    if (parsed.origin !== "https://github.com") return null;
    const prefix = `/${repo}/`;
    if (!parsed.pathname.toLowerCase().startsWith(prefix.toLowerCase()))
      return null;
    const path = parsed.pathname.slice(prefix.length);
    return (
      /^(?:actions\/runs\/\d+\/job|runs\/\d+\/jobs)\/([1-9]\d*)\/?$/.exec(
        path,
      )?.[1] ?? null
    );
  } catch {
    return null;
  }
}

export function fetchGithubCheckDetails(
  cwd: string,
  repo: string,
  jobId: string,
): Promise<GithubCheckDetails> {
  return invoke("git_github_check_details", { cwd, repo, jobId });
}

/** Backend owns the GitHub run/conclusion → state mapping; this only transports it. */
export function fetchGithubPrChecks(
  cwd: string,
  repo: string,
  number: number,
): Promise<GithubPrChecks> {
  return invoke<GithubPrChecks>("git_github_pr_checks", { cwd, repo, number });
}

export const CHECK_STATES: readonly GithubPrCheckState[] = [
  "fail",
  "pending",
  "cancel",
  "unknown",
  "pass",
  "skipping",
];

const CHECK_STATE_LABELS: Record<GithubPrCheckState, TextKey> = {
  pass: "inbox.checks.state.pass",
  fail: "inbox.checks.state.fail",
  pending: "inbox.checks.state.pending",
  cancel: "inbox.checks.state.cancel",
  unknown: "inbox.checks.state.unknown",
  skipping: "inbox.checks.state.skipping",
};

export function checkStateLabel(state: GithubPrCheckState): string {
  const label = t(CHECK_STATE_LABELS[state]);
  const tag = getLocaleTag();
  return label.charAt(0).toLocaleUpperCase(tag) + label.slice(1);
}

/** Group checks by outcome; the sort is stable so each group keeps its arrival order. */
export function sortChecks(checks: readonly GithubPrCheck[]): GithubPrCheck[] {
  return [...checks].sort(
    (a, b) => CHECK_STATES.indexOf(a.state) - CHECK_STATES.indexOf(b.state),
  );
}

export function countChecks(
  checks: readonly Pick<GithubPrCheck, "state">[],
): Record<GithubPrCheckState, number> {
  const counts: Record<GithubPrCheckState, number> = {
    pass: 0,
    fail: 0,
    pending: 0,
    skipping: 0,
    cancel: 0,
    unknown: 0,
  };
  for (const check of checks) counts[check.state] += 1;
  return counts;
}

/** One clause per non-zero state, so color never carries the counts alone. */
export function describeCheckCounts(
  counts: Record<GithubPrCheckState, number>,
): string | null {
  const parts = CHECK_STATES.filter((state) => counts[state] > 0).map(
    (state) => `${counts[state]} ${t(CHECK_STATE_LABELS[state])}`,
  );
  return parts.length > 0 ? parts.join(", ") : null;
}

export type GithubPrChecksOverall =
  | { kind: "loading"; description: string }
  | { kind: "error"; description: string }
  | { kind: "fail"; failed: number; description: string }
  | { kind: "pending"; description: string }
  | { kind: "neutral"; description: string }
  | { kind: "pass"; description: string };

/**
 * Overall indicator priority: load error, then fail, pending, cancel/unknown,
 * and success only when a pass exists beside passing/skipping checks. An empty
 * or skipping-only list stays neutral, and initial loading never reads as
 * "no checks".
 */
export function summarizePrChecks(input: {
  loading: boolean;
  error: string | null;
  checks: readonly GithubPrCheck[] | null;
}): GithubPrChecksOverall {
  if (input.loading) {
    return { kind: "loading", description: t("inbox.checks.loading") };
  }
  if (input.error) {
    const counts = input.checks ? countChecks(input.checks) : null;
    const saved = counts ? describeCheckCounts(counts) : null;
    return {
      kind: "error",
      description: saved
        ? t("inbox.checks.loadFailedSaved", { saved })
        : t("inbox.checks.loadFailed"),
    };
  }
  const checks = input.checks ?? [];
  const counts = countChecks(checks);
  if (counts.fail > 0) {
    return {
      kind: "fail",
      failed: counts.fail,
      description:
        describeCheckCounts(counts) ?? t("inbox.checks.noneReported"),
    };
  }
  if (counts.pending > 0) {
    return {
      kind: "pending",
      description:
        describeCheckCounts(counts) ?? t("inbox.checks.noneReported"),
    };
  }
  if (counts.cancel > 0 || counts.unknown > 0) {
    return {
      kind: "neutral",
      description:
        describeCheckCounts(counts) ?? t("inbox.checks.noneReported"),
    };
  }
  if (counts.pass > 0) {
    return {
      kind: "pass",
      description:
        describeCheckCounts(counts) ?? t("inbox.checks.noneReported"),
    };
  }
  // Empty or skipping-only: neutral, but the skipped count still gets said.
  return {
    kind: "neutral",
    description: describeCheckCounts(counts) ?? t("inbox.checks.noneReported"),
  };
}

/** Duration only exists when both stamps parse and complete at or after start. */
export function checkDuration(
  startedAt: string | null,
  completedAt: string | null,
): string | null {
  if (!startedAt || !completedAt) return null;
  const start = Date.parse(startedAt);
  const end = Date.parse(completedAt);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) {
    return null;
  }
  const totalSeconds = Math.round((end - start) / 1000);
  if (totalSeconds < 60)
    return t("inbox.duration.seconds", { seconds: totalSeconds });
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes < 60) {
    return t("inbox.duration.minutes", {
      minutes,
      seconds: String(seconds).padStart(2, "0"),
    });
  }
  const hours = Math.floor(minutes / 60);
  return t("inbox.duration.hours", {
    hours,
    minutes: String(minutes % 60).padStart(2, "0"),
  });
}

export function isHttpUrl(url: string | null): url is string {
  if (!url) return false;
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}
