import { describe, expect, it, vi } from "vitest";

vi.mock("expo-sqlite/kv-store", () => ({
  default: { getItemSync: () => null, setItem: async () => {} },
}));

import { homeProjectId, parsePlace } from "@/features/workspace/lastPlace";

const projects = [{ id: "p1" }, { id: "p2" }];
const monos = [
  { id: "m1", projectIds: ["gone", "p2"] },
  { id: "m2", projectIds: [] },
  { id: "m3", projectIds: ["gone"] },
];

describe("the project the app opens on", () => {
  it("is the last project visited while it is still on the phone", () => {
    expect(homeProjectId(projects, monos, { kind: "project", id: "p2" })).toBe("p2");
  });

  it("falls back to the first project when the last one is gone or nothing was saved", () => {
    expect(homeProjectId(projects, monos, { kind: "project", id: "gone" })).toBe("p1");
    expect(homeProjectId(projects, monos, null)).toBe("p1");
  });

  it("never lands on a Mono, even from an old saved Mono place", () => {
    const old = parsePlace(JSON.stringify({ kind: "mono", id: "m1" }));
    expect(old).toEqual({ kind: "mono", id: "m1" });
    // Its first project that is on the phone.
    expect(homeProjectId(projects, monos, old)).toBe("p2");
    // No project of its own on the phone, or none at all: the first project.
    for (const id of ["m2", "m3", "unknown"])
      expect(homeProjectId(projects, monos, { kind: "mono", id })).toBe("p1");
  });

  it("has nowhere to go without projects, so the app shows its start screen", () => {
    expect(homeProjectId([], monos, { kind: "mono", id: "m1" })).toBeUndefined();
    expect(homeProjectId([], [], null)).toBeUndefined();
  });

  it("reads what was saved, and nothing from garbage", () => {
    expect(parsePlace(JSON.stringify({ kind: "project", id: "p1" }))).toEqual({ kind: "project", id: "p1" });
    for (const raw of [null, "", "{", "[]", JSON.stringify({ kind: "tab", id: "x" }), JSON.stringify({ kind: "mono" })])
      expect(parsePlace(raw)).toBeNull();
  });
});
