import { describe, expect, it } from "vitest";
import { nextUnseenFinishedSessions } from "./sessionDone";

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
});
