import { describe, expect, it } from "vitest";
import { newSession, type Session } from "./session";
import {
  formatUsageLimitReset,
  USAGE_LIMIT_RESUME_GRACE_MS,
  usageLimitResumeDue,
  usageLimitFromError,
  resumeUsageLimitedSession,
} from "./usageLimit";

function limited(patch: Partial<Session> = {}): Session {
  return {
    ...newSession("codex", "/tmp/project"),
    usageLimit: { resetsAt: 10_000, resumeAtReset: true },
    ...patch,
  };
}

describe("usageLimitResumeDue", () => {
  it("waits for the reset plus a grace period", () => {
    expect(usageLimitResumeDue(limited(), 10_000)).toBe(false);
    expect(
      usageLimitResumeDue(limited(), 10_000 + USAGE_LIMIT_RESUME_GRACE_MS),
    ).toBe(true);
  });

  it("only resumes idle sessions the user armed", () => {
    const later = 10_000 + USAGE_LIMIT_RESUME_GRACE_MS;
    expect(usageLimitResumeDue(limited({ busy: true }), later)).toBe(false);
    expect(
      usageLimitResumeDue(limited({ usageLimit: { resetsAt: 10_000 } }), later),
    ).toBe(false);
    expect(
      usageLimitResumeDue(
        limited({ usageLimit: { resumeAtReset: true } }),
        later,
      ),
    ).toBe(false);
  });
});

describe("provider error fallback", () => {
  it.each([
    "You've hit your usage limit.",
    "Quota exceeded for this account",
    "insufficient_quota",
    "Credit balance is too low",
    "Monthly limit exceeded",
  ])("recognizes an exhausted account: %s", (message) => {
    expect(usageLimitFromError(message)).toEqual({});
  });

  it.each([
    "429: rate limit exceeded, retry in 3 seconds",
    "Server overloaded",
    "Context window exceeded",
    "Invalid API key",
    "Network timeout",
  ])("keeps other failures out of usage recovery: %s", (message) => {
    expect(usageLimitFromError(message)).toBeUndefined();
  });
});

it("only releases the outbox of an idle usage-limited session", () => {
  const session = limited({
    queueStatus: "paused",
    queuedMessages: [
      {
        id: "head",
        text: "Check tests",
        attachments: [],
        error: "Quota exceeded",
      },
    ],
  });
  const resumed = resumeUsageLimitedSession(session);
  expect(resumed.usageLimit).toBeUndefined();
  expect(resumed.queueStatus).toBe("active");
  expect(resumed.queuedMessages).toEqual([
    { id: "head", text: "Check tests", attachments: [], error: undefined },
  ]);
  expect(resumeUsageLimitedSession({ ...session, busy: true }).usageLimit).toBe(
    session.usageLimit,
  );
  const ordinary = { ...session, usageLimit: undefined };
  expect(resumeUsageLimitedSession(ordinary)).toBe(ordinary);
});

describe("formatUsageLimitReset", () => {
  it("shows the time and what is left", () => {
    const now = new Date(2026, 8, 25, 22, 34).getTime();
    const today = new Date(2026, 8, 25, 23, 50).getTime();
    expect(formatUsageLimitReset(today, now)).toMatch(/ · in 1h 16m$/);
    const tomorrow = new Date(2026, 8, 26, 3, 16).getTime();
    expect(formatUsageLimitReset(tomorrow, now)).toMatch(/26.* · in 4h 42m$/);
  });
});
