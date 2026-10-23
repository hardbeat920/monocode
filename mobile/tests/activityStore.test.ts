import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  dismissAllFinished,
  dismissFinished,
  markStartedHere,
  markStoppedHere,
  markViewing,
  resetSessionActivity,
  sessionActivity,
  trackSessions,
} from "@/features/sessions/activityStore";
import type { SessionSummary } from "@/features/workspace/types";

const session = (id: string, busy = false, extra: Partial<SessionSummary> = {}): SessionSummary => ({
  id,
  projectId: "p",
  title: id,
  harness: "claude",
  model: "opus",
  branch: "main",
  updatedAt: 0,
  busy,
  ...extra,
});

const ids = (list: { id: string }[]) => list.map((entry) => entry.id);

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(0);
  resetSessionActivity();
});

afterEach(() => {
  resetSessionActivity();
  vi.useRealTimers();
});

describe("session activity", () => {
  it("takes the first list as it is, with no arrivals or finishes", () => {
    trackSessions([session("a", true), session("b")]);
    expect(ids(sessionActivity().working)).toEqual(["a"]);
    expect(sessionActivity().finished).toEqual([]);
    expect(sessionActivity().arrivals).toEqual({});
  });

  it("keeps working sessions in the order they started as their times change", () => {
    trackSessions([session("a", true)]);
    trackSessions([session("b", true, { updatedAt: 2 }), session("a", true, { updatedAt: 1 })]);
    expect(ids(sessionActivity().working)).toEqual(["a", "b"]);
  });

  it("marks one started elsewhere as an arrival, and not one started here", () => {
    trackSessions([session("a"), session("b")]);
    markStartedHere("b");
    trackSessions([session("a", true), session("b", true)], 100);
    expect(sessionActivity().arrivals).toEqual({ a: 100 });
  });

  it("moves a finished session onto the deck, newest first", () => {
    trackSessions([session("a", true), session("b", true)]);
    trackSessions([session("a"), session("b", true)], 1);
    trackSessions([session("a"), session("b")], 2);
    expect(ids(sessionActivity().working)).toEqual([]);
    expect(sessionActivity().finished.map((entry) => entry.session.id)).toEqual(["b", "a"]);
  });

  it("counts waiting on the user as working", () => {
    trackSessions([session("a", true)]);
    trackSessions([session("a", false, { waiting: "approval" })]);
    expect(ids(sessionActivity().working)).toEqual(["a"]);
    expect(sessionActivity().finished).toEqual([]);
  });

  it("leaves off the deck one stopped here or open on screen", () => {
    trackSessions([session("a", true), session("b", true)]);
    markStoppedHere("a");
    const leave = markViewing("b");
    trackSessions([session("a"), session("b")]);
    expect(sessionActivity().finished).toEqual([]);
    leave();
  });

  it("takes a session off the deck as it works again", () => {
    trackSessions([session("a", true)]);
    trackSessions([session("a")]);
    trackSessions([session("a", true)]);
    expect(sessionActivity().finished).toEqual([]);
    expect(ids(sessionActivity().working)).toEqual(["a"]);
  });
});

describe("session activity, missing from the list", () => {
  it("keeps a finished session through a poll that leaves it out", () => {
    trackSessions([session("a", true)], 0);
    trackSessions([session("a")], 1_000);
    trackSessions([], 6_000);
    expect(sessionActivity().finished.map((entry) => entry.session.id)).toEqual(["a"]);
    trackSessions([session("a")], 11_000);
    trackSessions([], 16_000);
    trackSessions([], 50_000);
    expect(sessionActivity().finished).toHaveLength(1);
  });

  it("lets it go once it has been missing a while", () => {
    trackSessions([session("a", true)], 0);
    trackSessions([session("a")], 1_000);
    trackSessions([], 2_000);
    trackSessions([], 70_000);
    expect(sessionActivity().finished).toEqual([]);
  });

  it("expires a missing finished session without another catalog update", () => {
    trackSessions([session("a", true)]);
    trackSessions([session("a")]);
    trackSessions([]);

    vi.advanceTimersByTime(59_999);
    expect(sessionActivity().finished.map((entry) => entry.session.id)).toEqual(["a"]);
    vi.advanceTimersByTime(1);
    expect(sessionActivity().finished).toEqual([]);
  });

  it("cancels expiry when the session returns, and starts a fresh grace period if it disappears again", () => {
    trackSessions([session("a", true)]);
    trackSessions([session("a")]);
    trackSessions([]);
    vi.advanceTimersByTime(30_000);
    trackSessions([session("a")]);

    vi.advanceTimersByTime(60_000);
    expect(sessionActivity().finished).toHaveLength(1);
    trackSessions([]);
    vi.advanceTimersByTime(59_999);
    expect(sessionActivity().finished).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(sessionActivity().finished).toEqual([]);
  });

  it("expires each session according to when it first went missing", () => {
    trackSessions([session("a", true), session("b", true)]);
    trackSessions([session("a"), session("b")]);
    trackSessions([session("a")]);
    vi.advanceTimersByTime(10_000);
    trackSessions([]);

    vi.advanceTimersByTime(50_000);
    expect(sessionActivity().finished.map((entry) => entry.session.id)).toEqual(["a"]);
    vi.advanceTimersByTime(10_000);
    expect(sessionActivity().finished).toEqual([]);
  });

  it("cancels expiry timers when cards are dismissed or the activity is reset", () => {
    trackSessions([session("a", true), session("b", true), session("c", true, { projectId: "q" })]);
    trackSessions([session("a"), session("b"), session("c", false, { projectId: "q" })]);
    trackSessions([]);
    expect(vi.getTimerCount()).toBe(3);

    dismissFinished("a");
    expect(vi.getTimerCount()).toBe(2);
    dismissAllFinished("p");
    expect(vi.getTimerCount()).toBe(1);
    resetSessionActivity();
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(60_000);
    expect(sessionActivity().finished).toEqual([]);
  });
});
