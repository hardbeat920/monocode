import { describe, expect, it } from "vitest";
import {
  nextUnseenFinishedSessions,
  nextUnseenReplyCounts,
} from "./sessionDone";

describe("nextUnseenFinishedSessions", () => {
  it("marks a session done when it finishes while unfocused", () => {
    expect(
      nextUnseenFinishedSessions({
        previousBusyIds: new Set(["a"]),
        busyIds: new Set(),
        previousUnseenIds: new Set(),
        focusedSessionId: "b",
      }),
    ).toEqual(new Set(["a"]));
  });

  it("does not mark a session done when it finishes while focused", () => {
    expect(
      nextUnseenFinishedSessions({
        previousBusyIds: new Set(["a"]),
        busyIds: new Set(),
        previousUnseenIds: new Set(),
        focusedSessionId: "a",
      }),
    ).toEqual(new Set());
  });

  it("clears done when the session is focused", () => {
    expect(
      nextUnseenFinishedSessions({
        previousBusyIds: new Set(),
        busyIds: new Set(),
        previousUnseenIds: new Set(["a"]),
        focusedSessionId: "a",
      }),
    ).toEqual(new Set());
  });

  it("clears done when the session starts working again", () => {
    expect(
      nextUnseenFinishedSessions({
        previousBusyIds: new Set(),
        busyIds: new Set(["a"]),
        previousUnseenIds: new Set(["a"]),
        focusedSessionId: "b",
      }),
    ).toEqual(new Set());
  });

  it("keeps done on other sessions while one is focused", () => {
    expect(
      nextUnseenFinishedSessions({
        previousBusyIds: new Set(),
        busyIds: new Set(),
        previousUnseenIds: new Set(["a", "b"]),
        focusedSessionId: "a",
      }),
    ).toEqual(new Set(["b"]));
  });

  it("never marks sessions the user cannot look at", () => {
    // An unseen session stays loaded until focused; a worker never is.
    expect(
      nextUnseenFinishedSessions({
        previousBusyIds: new Set(["lead", "worker"]),
        busyIds: new Set(),
        previousUnseenIds: new Set(),
        focusedSessionId: "other",
        untrackedIds: new Set(["worker"]),
      }),
    ).toEqual(new Set(["lead"]));
  });

  it("drops untracked sessions that were already marked", () => {
    expect(
      nextUnseenFinishedSessions({
        previousBusyIds: new Set(),
        busyIds: new Set(),
        previousUnseenIds: new Set(["lead", "worker"]),
        focusedSessionId: "other",
        untrackedIds: new Set(["worker"]),
      }),
    ).toEqual(new Set(["lead"]));
  });
});

describe("nextUnseenReplyCounts", () => {
  const tracked = new Set(["a", "b"]);

  it("counts each reply finished while unfocused", () => {
    const once = nextUnseenReplyCounts({
      previousBusyIds: new Set(["a"]),
      busyIds: new Set(),
      previousCounts: new Map(),
      focusedSessionId: "b",
      trackedIds: tracked,
    });
    expect(once).toEqual(new Map([["a", 1]]));
    // Working again keeps the count; finishing adds to it.
    const working = nextUnseenReplyCounts({
      previousBusyIds: new Set(),
      busyIds: new Set(["a"]),
      previousCounts: once,
      focusedSessionId: "b",
      trackedIds: tracked,
    });
    expect(working).toEqual(new Map([["a", 1]]));
    expect(
      nextUnseenReplyCounts({
        previousBusyIds: new Set(["a"]),
        busyIds: new Set(),
        previousCounts: working,
        focusedSessionId: "b",
        trackedIds: tracked,
      }),
    ).toEqual(new Map([["a", 2]]));
  });

  it("does not count a reply finished while focused", () => {
    expect(
      nextUnseenReplyCounts({
        previousBusyIds: new Set(["a"]),
        busyIds: new Set(),
        previousCounts: new Map(),
        focusedSessionId: "a",
        trackedIds: tracked,
      }),
    ).toEqual(new Map());
  });

  it("clears the count when the session is focused", () => {
    expect(
      nextUnseenReplyCounts({
        previousBusyIds: new Set(),
        busyIds: new Set(),
        previousCounts: new Map([["a", 3]]),
        focusedSessionId: "a",
        trackedIds: tracked,
      }),
    ).toEqual(new Map());
  });

  it("ignores sessions that are not tracked", () => {
    expect(
      nextUnseenReplyCounts({
        previousBusyIds: new Set(["c"]),
        busyIds: new Set(),
        previousCounts: new Map([["d", 2]]),
        trackedIds: tracked,
      }),
    ).toEqual(new Map());
  });
});
