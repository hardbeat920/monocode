import { describe, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import {
  mobileUsageLimit,
  formatUsageLimitReset,
  validateMobileUsageLimit,
} from "../../sessions/model/usageLimit";
import { recoverMobileUsage } from "./mobileUsageLimit";

const session = () => ({ ...newSession("codex", "/fixture"), usageLimit: {} });
describe("desktop-owned mobile usage recovery", () => {
  it("carries actual multi-day reset data and the desktop's reset wording", () => {
    const now = Date.now();
    const resetsAt = now + 7 * 24 * 3600_000;
    const limited = {
      ...session(),
      usageLimit: { resetsAt, resumeAtReset: true },
    };
    expect(mobileUsageLimit(limited, now)).toMatchObject({
      resetsAt,
      resumeAtReset: true,
      canResume: false,
      canAutoResume: true,
      resetLabel: `Resets ${formatUsageLimitReset(resetsAt, now)}`,
    });
    expect(() =>
      validateMobileUsageLimit(
        limited,
        mobileUsageLimit(limited)!.token,
        false,
      ),
    ).toThrow("after the allowance resets");
  });
  it("allows an unknown-reset manual attempt without inventing a schedule, and rejects remote or busy sessions", () => {
    const limited = session();
    expect(mobileUsageLimit(limited)!.limitation).toContain(
      "preference is kept only until the session closes or the app restarts",
    );
    expect(mobileUsageLimit(limited)).toMatchObject({
      canResume: true,
      canAutoResume: false,
      resetLabel: "Reset time unknown",
    });
    for (const patch of [{ cwd: "remote://host/project" }, { busy: true }]) {
      const blocked = { ...limited, ...patch };
      expect(() =>
        validateMobileUsageLimit(
          blocked,
          mobileUsageLimit(blocked)!.token,
          false,
        ),
      ).toThrow();
    }
    expect(mobileUsageLimit({ ...limited, usageLimit: undefined })).toBeNull();
  });
  it.each([undefined, -60_000, 60_000])(
    "matches desktop automatic controls for reset offset %s, armed or unarmed",
    (offset) => {
      const now = Date.now();
      for (const resumeAtReset of [false, true]) {
        const limited = {
          ...session(),
          usageLimit: {
            resetsAt: offset === undefined ? undefined : now + offset,
            resumeAtReset,
          },
        };
        const state = mobileUsageLimit(limited, now)!;
        expect(state.canAutoResume).toBe(offset !== undefined && offset > 0);
        expect(state.canResume).toBe(offset === undefined || offset <= 0);
        if (!state.canAutoResume)
          expect(() =>
            validateMobileUsageLimit(limited, state.token, true),
          ).toThrow();
      }
    },
  );
  it("calls the existing desktop continuation once, permits retry after failure, and rejects stale or removed state", async () => {
    const limited = session();
    let finish!: (value: boolean) => void;
    const resume = vi.fn(
      () =>
        new Promise<boolean>((resolve) => {
          finish = resolve;
        }),
    );
    const options = {
      session: limited,
      token: mobileUsageLimit(limited)!.token,
      automatic: false,
      enabled: undefined,
      pending: new Set<string>(),
      resume,
      setAutomatic: vi.fn(),
    };
    const request = recoverMobileUsage(options);
    await expect(recoverMobileUsage(options)).rejects.toThrow(
      "already in progress",
    );
    finish(false);
    await expect(request).rejects.toThrow("try again");
    expect(options.pending.size).toBe(0);
    resume.mockResolvedValueOnce(true);
    await recoverMobileUsage(options);
    expect(resume).toHaveBeenCalledTimes(2);
    expect(resume).toHaveBeenCalledWith(limited.id);
    await expect(
      recoverMobileUsage({
        ...options,
        session: { ...limited, providerAccountId: "another-account" },
      }),
    ).rejects.toThrow("changed");

    await expect(
      recoverMobileUsage({
        ...options,
        session: { ...limited, model: "different-model" },
      }),
    ).rejects.toThrow("changed");
    await expect(
      recoverMobileUsage({ ...options, session: undefined }),
    ).rejects.toThrow("no longer available");
    await expect(
      recoverMobileUsage({
        ...options,
        session: { ...limited, usageLimit: undefined },
      }),
    ).rejects.toThrow("no longer paused");
    await expect(
      recoverMobileUsage({
        ...options,
        session: { ...limited, usageLimit: { resetsAt: 1 } },
      }),
    ).rejects.toThrow("changed");
  });
  it("enables and disables the desktop scheduler preference, with compare-and-set semantics across devices", async () => {
    const limited = {
      ...session(),
      usageLimit: { resetsAt: Date.now() + 60_000, resumeAtReset: false },
    };
    const setAutomatic = vi.fn((_id, enabled) => {
      limited.usageLimit.resumeAtReset = enabled;
    });
    const options = {
      session: limited,
      token: mobileUsageLimit(limited)!.token,
      automatic: true,
      enabled: true,
      pending: new Set<string>(),
      resume: vi.fn(),
      setAutomatic,
    };
    await recoverMobileUsage(options);
    expect(mobileUsageLimit(limited)!.resumeAtReset).toBe(true);
    await expect(recoverMobileUsage(options)).rejects.toThrow("changed");
    await recoverMobileUsage({
      ...options,
      token: mobileUsageLimit(limited)!.token,
      enabled: false,
    });
    expect(setAutomatic.mock.calls.map((call) => call[1])).toEqual([
      true,
      false,
    ]);
    expect(options.resume).not.toHaveBeenCalled();
  });
});
