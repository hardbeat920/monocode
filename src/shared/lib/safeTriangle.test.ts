import { describe, expect, it } from "vitest";
import { graceArea, inGrace } from "./safeTriangle";

const submenu = { left: 200, right: 400, top: 0, bottom: 300 };

describe("safe triangle", () => {
  it("keeps diagonal moves toward the submenu inside the grace area", () => {
    const grace = graceArea({ x: 190, y: 50 }, submenu, 0)!;
    expect(inGrace({ x: 195, y: 70 }, grace, 0)).toBe(true);
  });

  it("expires once the pointer lingers", () => {
    const grace = graceArea({ x: 190, y: 50 }, submenu, 0)!;
    expect(inGrace({ x: 195, y: 70 }, grace, 1000)).toBe(false);
  });

  it("gives no grace toward an unmeasured submenu", () => {
    const empty = { left: 0, right: 0, top: 0, bottom: 0 };
    expect(graceArea({ x: 190, y: 50 }, empty, 0)).toBeNull();
  });

  it("releases moves that head away from the submenu", () => {
    const grace = graceArea({ x: 190, y: 50 }, submenu, 0)!;
    expect(inGrace({ x: 150, y: 90 }, grace, 0)).toBe(false);
  });

  it("aims at the right edge when the submenu flipped left", () => {
    const grace = graceArea({ x: 410, y: 50 }, submenu, 0)!;
    expect(grace.top.x).toBe(400);
    expect(inGrace({ x: 405, y: 70 }, grace, 0)).toBe(true);
  });
});
