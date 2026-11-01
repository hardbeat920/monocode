import { describe, expect, it } from "vitest";

import {
  finishedFromMono,
  groupSize,
  monosForProject,
  sameOrigin,
  searchMonos,
  sessionOrigin,
} from "@/features/sessions/sessionMonos";
import type { Mono, SessionSummary } from "@/features/workspace/types";

const mono = (id: string, projectIds: string[], extra: Partial<Mono> = {}): Mono => ({
  id,
  name: id.toUpperCase(),
  mascot: "robot",
  color: "#ff8800",
  projectIds,
  unread: 0,
  status: "idle",
  lastMessage: "",
  lastActiveAt: 0,
  ...extra,
});
const session = (id: string, extra: Partial<SessionSummary> = {}): SessionSummary => ({
  id,
  projectId: "p1",
  title: id,
  harness: "claude",
  model: "m",
  branch: "main",
  updatedAt: 1,
  ...extra,
});
const projects = [{ id: "p1" }, { id: "p2" }, { id: "p3" }];

describe("the Monos pinned on a project's Sessions page", () => {
  const monos = [
    mono("solo", ["p1"], { lastActiveAt: 5 }),
    mono("both", ["p1", "p2"], { lastActiveAt: 9 }),
    mono("other", ["p3"]),
    mono("loose", []),
    mono("loose2", [], { lastActiveAt: 3 }),
  ];

  it("lists the Monos assigned to the project, the latest heard from first", () => {
    expect(monosForProject(monos, projects, "p1").assigned.map((m) => m.id)).toEqual(["both", "solo"]);
    expect(monosForProject(monos, projects, "p3").assigned.map((m) => m.id)).toEqual(["other"]);
  });

  it("lists a Mono assigned to several projects in each of them", () => {
    for (const id of ["p1", "p2"])
      expect(monosForProject(monos, projects, id).assigned.map((m) => m.id)).toContain("both");
    expect(monosForProject(monos, projects, "p3").assigned.map((m) => m.id)).not.toContain("both");
  });

  it("pins Monos with no project as Unassigned on the first project's page only", () => {
    expect(monosForProject(monos, projects, "p1").unassigned.map((m) => m.id)).toEqual(["loose2", "loose"]);
    expect(monosForProject(monos, projects, "p2").unassigned).toEqual([]);
    expect(monosForProject(monos, projects, "p3").unassigned).toEqual([]);
    // Never twice: unassigned Monos are not "assigned" anywhere.
    expect(monosForProject(monos, projects, "p1").assigned.map((m) => m.id)).not.toContain("loose");
  });

  it("has no unassigned group while there are no projects, and counts both groups", () => {
    expect(monosForProject(monos, [], "p1").unassigned).toEqual([]);
    expect(groupSize(monosForProject(monos, projects, "p1"))).toBe(4);
    expect(groupSize(monosForProject([], projects, "p1"))).toBe(0);
  });

  it("narrows to the search by name or last line", () => {
    const group = monosForProject(
      [mono("alpha", ["p1"], { lastMessage: "Shipped the fix" }), mono("beta", ["p1"]), mono("gamma", [])],
      projects,
      "p1",
    );
    expect(searchMonos(group, "").assigned).toHaveLength(2);
    expect(searchMonos(group, "alp").assigned.map((m) => m.id)).toEqual(["alpha"]);
    expect(searchMonos(group, "shipped").assigned.map((m) => m.id)).toEqual(["alpha"]);
    expect(searchMonos(group, "gam").unassigned.map((m) => m.id)).toEqual(["gamma"]);
    expect(groupSize(searchMonos(group, "nothing"))).toBe(0);
  });
});

describe("who started a session", () => {
  const known = mono("tg", ["p1"], { name: "TaskGiver" });
  const away = mono("away", ["p3"]);
  const all = new Map([known, away].map((m) => [m.id, m]));
  const shown = new Set(["tg"]);

  it("shows the Mono that started it, tappable when its chat is on the phone", () => {
    expect(sessionOrigin({ monoId: "tg" }, all, shown)).toEqual({ kind: "mono", mono: known, openable: true });
  });

  it("names a Mono whose projects are not picked, without a way into its chat", () => {
    expect(sessionOrigin({ monoId: "away" }, all, shown)).toEqual({ kind: "mono", mono: away, openable: false });
  });

  it("marks a Mono the desktop no longer lists as removed", () => {
    expect(sessionOrigin({ monoId: "gone" }, all, shown)).toEqual({ kind: "removed" });
  });

  it("has no chip without a monoId: by hand, or adopted, the desktop can't tell", () => {
    expect(sessionOrigin({}, all, shown)).toBeNull();
    expect(sessionOrigin({ monoId: undefined }, all, shown)).toBeNull();
    expect(sessionOrigin({ monoId: "" }, all, shown)).toBeNull();
  });

  it("compares origins by what the chip draws", () => {
    const a = sessionOrigin({ monoId: "tg" }, all, shown);
    expect(sameOrigin(a, { kind: "mono", mono: { ...known, unread: 3, lastMessage: "x" }, openable: true })).toBe(true);
    expect(sameOrigin(a, { kind: "mono", mono: { ...known, name: "Renamed" }, openable: true })).toBe(false);
    expect(sameOrigin(a, { kind: "mono", mono: { ...known, color: "#000" }, openable: true })).toBe(false);
    expect(sameOrigin(a, { kind: "mono", mono: known, openable: false })).toBe(false);
    expect(sameOrigin(a, { kind: "removed" })).toBe(false);
    expect(sameOrigin({ kind: "removed" }, { kind: "removed" })).toBe(true);
    expect(sameOrigin(a, null)).toBe(false);
    expect(sameOrigin(null, null)).toBe(true);
  });
});

describe("a Mono's finished sessions", () => {
  it("takes its own sessions that are neither working nor waiting", () => {
    const list = [
      session("done", { monoId: "tg" }),
      session("busy", { monoId: "tg", busy: true }),
      session("asks", { monoId: "tg", waiting: "question" }),
      session("theirs", { monoId: "other" }),
      session("manual"),
    ];
    expect(finishedFromMono(list, "tg").map((s) => s.id)).toEqual(["done"]);
    expect(finishedFromMono(list, "nobody")).toEqual([]);
  });
});
