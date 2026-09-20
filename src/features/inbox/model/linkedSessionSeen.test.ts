// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import {
  linkedSessionSeenAt,
  markLinkedSessionUpdateSeen,
} from "./linkedSessionSeen";

describe("linked session seen snapshots", () => {
  beforeEach(() => localStorage.clear());

  it("remembers the newest acknowledged remote update per session", () => {
    markLinkedSessionUpdateSeen("session-1", 200);
    markLinkedSessionUpdateSeen("session-1", 150);
    markLinkedSessionUpdateSeen("session-2", 300);

    expect(linkedSessionSeenAt("session-1")).toBe(200);
    expect(linkedSessionSeenAt("session-2")).toBe(300);
  });
});
