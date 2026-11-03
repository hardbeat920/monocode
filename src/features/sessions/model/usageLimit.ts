import { formatResetDuration } from "../../providers/model/rateLimits";
import type { Session, UsageLimit } from "./session";

/** A fallback for adapters that report exhausted accounts as plain errors. */
export function usageLimitFromError(message: string): UsageLimit | undefined {
  return /(?:usage|spending|monthly|weekly|daily) limit (?:reached|exceeded)|(?:quota|credits?) (?:exceeded|exhausted|depleted)|insufficient[_ ](?:quota|credits)|hit your (?:usage )?limit|credit balance is too low/i.test(
    message,
  )
    ? {}
    : undefined;
}

/** Release the stopped outbox only when the user chooses to recover. */
export function resumeUsageLimitedSession(session: Session): Session {
  if (!session.usageLimit || session.busy) return session;
  return {
    ...session,
    usageLimit: undefined,
    queueStatus: session.queuedMessages?.length ? "active" : undefined,
    queuedMessages: session.queuedMessages?.map((message) => ({
      ...message,
      error: undefined,
    })),
  };
}

/**
 * Point a usage-limited session at another account of its provider. When its
 * native transcript was carried to that account, the same provider session
 * resumes there. Otherwise the next turn starts a new provider thread from a
 * handoff brief; the limited account cannot be asked for a recap.
 */
export function withUsageLimitAccount(
  session: Session,
  accountId: string,
  carried: boolean,
): Session {
  if (carried) return { ...session, providerAccountId: accountId };
  return {
    ...session,
    providerAccountId: accountId,
    providerSessionId: undefined,
    pendingSwitch: {
      ...(session.pendingSwitch ?? {
        from: session.harness,
        fromModel: session.model,
        fromSettings: session.modelSettings,
        fromProviderSessionId: session.providerSessionId,
        fromProviderAccountId: session.providerAccountId,
      }),
      skipOutgoingRecap: true,
    },
  };
}

/** Providers can still refuse right at the reset; give them a moment. */
export const USAGE_LIMIT_RESUME_GRACE_MS = 30_000;

/** "3:16 AM · in 4h 42m" today, "Sep 26, 3:16 AM · in 1d 4h" later. */
export function formatUsageLimitReset(resetsAt: number, now: number): string {
  const reset = new Date(resetsAt);
  const sameDay = reset.toDateString() === new Date(now).toDateString();
  const when = reset.toLocaleString(undefined, {
    ...(sameDay ? {} : { month: "short", day: "numeric" }),
    hour: "numeric",
    minute: "2-digit",
  });
  return `${when} · in ${formatResetDuration(resetsAt - now)}`;
}

/** Idle, armed, and past its reset: time to send the continue turn. */
export function usageLimitResumeDue(session: Session, now: number): boolean {
  const limit = session.usageLimit;
  if (!limit?.resumeAtReset || limit.resetsAt == null || session.busy) {
    return false;
  }
  return now >= limit.resetsAt + USAGE_LIMIT_RESUME_GRACE_MS;
}

/** Desktop-owned contract for the paired phone. Never infer reset windows on the phone. */
export type MobileUsageLimit = {
  token: string;
  resetsAt?: number;
  resumeAtReset: boolean;
  canResume: boolean;
  canAutoResume: boolean;
  resetLabel: string;
  limitation: string;
};

export function usageLimitWaiting(limit: UsageLimit, now: number): boolean {
  return limit.resetsAt != null && limit.resetsAt > now;
}

export function mobileUsageLimit(
  session: Session,
  now = Date.now(),
): MobileUsageLimit | null {
  const limit = session.usageLimit;
  if (!limit) return null;
  const supported =
    !session.cwd.startsWith("remote://") && !session.worktreeRemoved;
  const waiting = usageLimitWaiting(limit, now);
  return {
    token: JSON.stringify([
      session.id,
      session.harness,
      session.model,
      session.providerAccountId,
      session.providerSessionId,
      session.blocks.length,
      session.blocks[session.blocks.length - 1]?.id,
      limit,
    ]),
    resetsAt: limit.resetsAt,
    resumeAtReset: !!limit.resumeAtReset,
    canResume: supported && !session.busy && !waiting,
    canAutoResume: supported && !session.busy && waiting,
    resetLabel:
      limit.resetsAt == null
        ? "Reset time unknown"
        : waiting
          ? `Resets ${formatUsageLimitReset(limit.resetsAt, now)}`
          : "Limit has reset",
    limitation: supported
      ? "Automatic continuation requires Monocode running with this session loaded. This preference is kept only until the session closes or the app restarts."
      : session.worktreeRemoved
        ? "This session’s working copy was removed. Open it on desktop to recover."
        : "Continue this session on its remote desktop. Mobile usage-limit recovery is unavailable for this session.",
  };
}

export function validateMobileUsageLimit(
  session: Session,
  token: unknown,
  automatic: boolean,
): MobileUsageLimit {
  const state = mobileUsageLimit(session);
  if (!state)
    throw new Error("This session is no longer paused by a usage limit");
  if (state.token !== token)
    throw new Error(
      "This session changed. Wait for it to refresh and try again.",
    );
  if (!(automatic ? state.canAutoResume : state.canResume))
    throw new Error(
      state.resetsAt != null && state.resetsAt > Date.now() && !automatic
        ? `${state.resetLabel}. Enable Resume at reset or try again after the allowance resets.`
        : "This session cannot continue right now. Check it on desktop and try again.",
    );
  return state;
}
