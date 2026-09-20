import { describe, expect, it } from "vitest";
import {
  MASCOT_GRID,
  PROJECT_MASCOTS,
  mascotPath,
  projectMascot,
} from "./projectMascots";

describe("projectMascots", () => {
  it("keeps every sprite on the shared grid", () => {
    expect(PROJECT_MASCOTS).toHaveLength(10);
    for (const mascot of PROJECT_MASCOTS) {
      for (const frame of [mascot.rest, mascot.talk]) {
        expect(frame).toHaveLength(MASCOT_GRID);
        for (const row of frame) {
          expect(row).toMatch(new RegExp(`^[#.]{${MASCOT_GRID}}$`));
        }
      }
      expect(mascot.restPath).not.toBe("");
      expect(mascot.talkPath).not.toBe("");
      expect(mascot.talkPath).not.toBe(mascot.restPath);
    }
  });

  it("merges filled runs into one rect each", () => {
    expect(mascotPath(["##..###."])).toBe("M0 0h2v1h-2zM4 0h3v1h-3z");
    expect(mascotPath(["........"])).toBe("");
  });

  it("picks the same mascot for the same project", () => {
    expect(projectMascot("~/code/monocode")).toBe(
      projectMascot("~/code/monocode"),
    );
  });

  it("honors an explicit pick and ignores unknown names", () => {
    expect(projectMascot("alpha", "ghost").name).toBe("ghost");
    expect(projectMascot("alpha", "nope").name).toBe(
      projectMascot("alpha").name,
    );
    expect(projectMascot("alpha", null).name).toBe(projectMascot("alpha").name);
  });

  it("spreads projects across the roster", () => {
    const names = new Set(
      ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta"].map(
        (project) => projectMascot(project).name,
      ),
    );
    expect(names.size).toBeGreaterThan(3);
  });
});
