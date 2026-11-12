// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Session } from "../model/session";
import { newSession } from "../model/session";
import { useTurnRateLimitsRefresh } from "./useTurnRateLimitsRefresh";
import * as rateLimitsCache from "../../providers/model/rateLimitsCache";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function TestHarness({
  sessions,
  busySessionIds,
}: {
  sessions: Session[];
  busySessionIds: ReadonlySet<string>;
}) {
  useTurnRateLimitsRefresh(sessions, busySessionIds);
  return null;
}

describe("useTurnRateLimitsRefresh", () => {
  it("schedules a rate-limit refresh when a session finishes its turn", () => {
    const scheduleSpy = vi.spyOn(
      rateLimitsCache,
      "scheduleRateLimitsAutoRefresh",
    );
    const session = {
      ...newSession("claude", "/repo"),
      id: "sess-1",
      providerAccountId: "work-account",
    };

    // Mount while busy
    act(() => {
      root.render(
        createElement(TestHarness, {
          sessions: [session],
          busySessionIds: new Set(["sess-1"]),
        }),
      );
    });

    expect(scheduleSpy).not.toHaveBeenCalled();

    // Turn completes: busySessionIds becomes empty
    act(() => {
      root.render(
        createElement(TestHarness, {
          sessions: [session],
          busySessionIds: new Set(),
        }),
      );
    });

    expect(scheduleSpy).toHaveBeenCalledTimes(1);
    expect(scheduleSpy).toHaveBeenCalledWith("claude", "work-account");
  });

  it("ignores harnesses that do not support rate limits", () => {
    const scheduleSpy = vi.spyOn(
      rateLimitsCache,
      "scheduleRateLimitsAutoRefresh",
    );
    const session = {
      ...newSession("pi", "/repo"),
      id: "sess-2",
    };

    act(() => {
      root.render(
        createElement(TestHarness, {
          sessions: [session],
          busySessionIds: new Set(["sess-2"]),
        }),
      );
    });

    act(() => {
      root.render(
        createElement(TestHarness, {
          sessions: [session],
          busySessionIds: new Set(),
        }),
      );
    });

    expect(scheduleSpy).not.toHaveBeenCalled();
  });

  it("refreshes the provider that ran the turn even if the session switched model while busy", () => {
    const scheduleSpy = vi.spyOn(
      rateLimitsCache,
      "scheduleRateLimitsAutoRefresh",
    );
    const initialSession = {
      ...newSession("claude", "/repo"),
      id: "sess-3",
      providerAccountId: "work-account",
    };

    // Mount while busy running Claude turn
    act(() => {
      root.render(
        createElement(TestHarness, {
          sessions: [initialSession],
          busySessionIds: new Set(["sess-3"]),
        }),
      );
    });

    // User switches to Codex while turn is still busy
    const switchedSession = {
      ...initialSession,
      harness: "codex" as const,
      providerAccountId: undefined,
    };
    act(() => {
      root.render(
        createElement(TestHarness, {
          sessions: [switchedSession],
          busySessionIds: new Set(["sess-3"]),
        }),
      );
    });

    // Turn completes
    act(() => {
      root.render(
        createElement(TestHarness, {
          sessions: [switchedSession],
          busySessionIds: new Set(),
        }),
      );
    });

    // Should refresh Claude and work-account that ran the turn, not Codex
    expect(scheduleSpy).toHaveBeenCalledTimes(1);
    expect(scheduleSpy).toHaveBeenCalledWith("claude", "work-account");
  });
});

