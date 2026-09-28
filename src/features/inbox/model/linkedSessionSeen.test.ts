// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import {
  linkedSessionSeenAt,
  markLinkedSessionUpdateSeen,
} from "./linkedSessionSeen";

const pull = { kind: "pr" as const, repo: "acme/app", number: 42 };
const issue = { kind: "issue" as const, repo: "acme/app", number: 8 };

describe("linked session seen snapshots", () => {
  beforeEach(() => localStorage.clear());

  it("remembers the newest acknowledged remote update per linked item", () => {
    markLinkedSessionUpdateSeen("session-1", pull, 200);
    markLinkedSessionUpdateSeen("session-1", pull, 150);
    markLinkedSessionUpdateSeen("session-1", issue, 180);
    markLinkedSessionUpdateSeen("session-2", pull, 300);

    expect(linkedSessionSeenAt("session-1", pull)).toBe(200);
    expect(linkedSessionSeenAt("session-1", issue)).toBe(180);
    expect(linkedSessionSeenAt("session-2", pull)).toBe(300);
  });
});
