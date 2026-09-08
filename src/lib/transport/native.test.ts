import { describe, expect, it } from "vitest";
import { canUseNativeSocket, parseNativeWsEvent } from "./native";

describe("native companion socket", () => {
  it("is off outside Tauri (tests, browsers)", () => {
    expect(canUseNativeSocket()).toBe(false);
  });

  it("parses tagged client events and rejects junk", () => {
    expect(
      parseNativeWsEvent({ kind: "message", id: "abc", data: "{\"ok\":true}" }),
    ).toEqual({ kind: "message", id: "abc", data: "{\"ok\":true}" });
    expect(
      parseNativeWsEvent({ kind: "close", id: "abc", code: 1000, reason: "bye" }),
    ).toEqual({ kind: "close", id: "abc", code: 1000, reason: "bye" });
    expect(
      parseNativeWsEvent({ kind: "error", id: "abc", message: "timeout" }),
    ).toEqual({ kind: "error", id: "abc", message: "timeout" });
    expect(parseNativeWsEvent({ kind: "message", id: "abc" })).toBeNull();
    expect(parseNativeWsEvent({ kind: "open", id: "abc" })).toEqual({
      kind: "open",
      id: "abc",
    });
    expect(parseNativeWsEvent({ kind: "noop", id: "abc" })).toBeNull();
    expect(parseNativeWsEvent(null)).toBeNull();
  });
});
