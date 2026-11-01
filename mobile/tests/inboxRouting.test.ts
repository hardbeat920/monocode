import { describe, expect, it, vi } from "vitest";

import { initials } from "@/features/inbox/model";
import { inboxTarget } from "@/features/inbox/routing";
import { defaultProject, startParams } from "@/features/inbox/startAgent";
import { NOTIFICATION_CATEGORIES } from "@/features/settings/notifications";

vi.mock("expo-sqlite/kv-store", () => ({ default: { getItemSync: () => null, setItem: vi.fn() } }));

describe("notification routing", () => {
  it("opens the item for {kind: inbox, key}", () => {
    expect(inboxTarget({ kind: "inbox", key: "github:a/b#1" })).toEqual({
      pathname: "/inbox/item",
      params: { key: "github:a/b#1" },
    });
  });

  it.each([
    ["no data", undefined],
    ["a session's data", { projectId: "p", sessionId: "s" }],
    ["another kind", { kind: "mono", key: "k" }],
    ["no key", { kind: "inbox" }],
    ["an empty key", { kind: "inbox", key: "" }],
    ["a key that isn't text", { kind: "inbox", key: 7 }],
    ["a string", "inbox"],
  ])("leaves %s to the other routes", (_name, data) => {
    expect(inboxTarget(data)).toBeNull();
  });

  it("has a push category beside the agent ones", () => {
    expect(NOTIFICATION_CATEGORIES.map((category) => category.id)).toEqual([
      "agentInput",
      "agentFinished",
      "monoMessages",
      "inbox",
    ]);
  });
});

describe("starting an agent", () => {
  const projects = [{ id: "a" }, { id: "b" }];

  it("prefers the item's own project, then the one in view, then the first", () => {
    expect(defaultProject(projects, "b", "a")).toBe("b");
    expect(defaultProject(projects, undefined, "b")).toBe("b");
    expect(defaultProject(projects, "gone", undefined)).toBe("a");
    expect(defaultProject([], "a", "a")).toBeUndefined();
  });

  it("sends only what was chosen", () => {
    expect(startParams({ key: "k", projectId: "a", mode: "ask", text: "  " })).toEqual({
      key: "k",
      projectId: "a",
      mode: "ask",
    });
    expect(
      startParams({ key: "k", projectId: "a", mode: "work", harness: "claude", model: "m", text: " Fix it " }),
    ).toEqual({ key: "k", projectId: "a", mode: "work", harness: "claude", model: "m", text: "Fix it" });
  });
});

describe("avatars", () => {
  it("takes two letters from a name", () => {
    expect(initials("maren-k")).toBe("MK");
    expect(initials("sam")).toBe("SA");
    expect(initials("Priya N.")).toBe("PN");
    expect(initials("x")).toBe("X");
    expect(initials("12")).toBe("?");
  });
});
