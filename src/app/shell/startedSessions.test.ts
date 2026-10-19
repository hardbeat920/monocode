import { describe, expect, it } from "vitest";
import { groupStartedSessions, startedSessionCaller } from "./Sidebar";

const rows = (...ids: string[]) => ids.map((id) => ({ id }));

describe("groupStartedSessions", () => {
  it("hangs a session started through the app CLI under its caller", () => {
    const { byParent, childIds } = groupStartedSessions(
      rows(
        "4f1c-9a2b",
        "app-4f1c-9a2b-review-ui",
        "app-4f1c-9a2b-review_api-2",
        "other",
      ),
    );
    expect(byParent.get("4f1c-9a2b")?.map((row) => row.id)).toEqual([
      "app-4f1c-9a2b-review-ui",
      "app-4f1c-9a2b-review_api-2",
    ]);
    expect([...childIds].sort()).toEqual([
      "app-4f1c-9a2b-review-ui",
      "app-4f1c-9a2b-review_api-2",
    ]);
  });

  it("joins a launch's own launches to the top-level session", () => {
    const { byParent } = groupStartedSessions(
      rows("lead", "app-lead-a", "app-app-lead-a-b"),
    );
    expect(byParent.get("lead")?.map((row) => row.id)).toEqual([
      "app-lead-a",
      "app-app-lead-a-b",
    ]);
    expect(byParent.has("app-lead-a")).toBe(false);
  });

  it("leaves a launch whose caller is not listed where it is", () => {
    const { byParent, childIds } = groupStartedSessions(
      rows("app-mono-session-1-task", "plain"),
    );
    expect(byParent.size).toBe(0);
    expect(childIds.size).toBe(0);
  });

  it("finds a launch's caller only among the given sessions", () => {
    const ids = new Set(["ab3c-98dc", "plain"]);
    expect(startedSessionCaller("app-ab3c-98dc-3a64-6702", ids)).toBe(
      "ab3c-98dc",
    );
    expect(startedSessionCaller("app-mono-1-task", ids)).toBeUndefined();
    expect(startedSessionCaller("plain", ids)).toBeUndefined();
  });
});
