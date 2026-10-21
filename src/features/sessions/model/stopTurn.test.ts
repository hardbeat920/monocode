import { describe, expect, it } from "vitest";
import { newSession, type Session } from "./session";
import { stopSessionTurn } from "./stopTurn";
import {
  USAGE_LIMIT_RESUME_GRACE_MS,
  usageLimitResumeDue,
} from "./usageLimit";

describe("stopSessionTurn", () => {
  it("disarms resume-at-reset so Stop prevents an automatic resume", () => {
    const armed: Session = {
      ...newSession("codex", "/tmp/project"),
      busy: true,
      usageLimit: { resetsAt: 10_000, resumeAtReset: true },
    };
    const stopped = stopSessionTurn(armed);
    expect(stopped.busy).toBe(false);
    expect(stopped.usageLimit).toEqual({
      resetsAt: 10_000,
      resumeAtReset: false,
    });
    expect(
      usageLimitResumeDue(stopped, 10_000 + USAGE_LIMIT_RESUME_GRACE_MS),
    ).toBe(false);
  });
});
