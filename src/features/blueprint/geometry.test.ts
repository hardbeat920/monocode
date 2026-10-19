import { describe, expect, it } from "vitest";
import { borderPoint, sideOf, spreadBoxes } from "./geometry";

// Arrows attach where the line between two boxes leaves each box, so the dot
// slides along the border as boxes move: below means bottom, right means
// right, and stadium ends follow the curve, never the invisible corner.
describe("borderPoint", () => {
  const box = { x: 0, y: 0, width: 200, height: 60 };

  it("exits the bottom edge toward a box straight below", () => {
    const p = borderPoint(box, "rectangle", { x: 100, y: 500 });
    expect(p).toEqual({ x: 100, y: 60 });
    expect(sideOf(box, p)).toBe("bottom");
  });

  it("exits the right edge toward a box to the right", () => {
    const p = borderPoint(box, "rectangle", { x: 900, y: 30 });
    expect(p).toEqual({ x: 200, y: 30 });
    expect(sideOf(box, p)).toBe("right");
  });

  it("slides along the edge as the other box moves diagonally", () => {
    const p = borderPoint(box, "rectangle", { x: 160, y: 90 });
    expect(p.y).toBe(60);
    expect(p.x).toBeCloseTo(130);
  });

  it("stays inside the border by the inset", () => {
    expect(borderPoint(box, "rectangle", { x: 100, y: 500 }, 2)).toEqual({
      x: 100,
      y: 58,
    });
  });

  it("follows a stadium's rounded end instead of the corner", () => {
    // Shallow angle up-right from the center: it leaves through the right
    // half-circle, where a rectangle would have its corner.
    const p = borderPoint(box, "stadium", { x: 1100, y: -170 });
    const end = { x: 170, y: 30 }; // right end circle center, radius 30
    expect(Math.hypot(p.x - end.x, p.y - end.y)).toBeCloseTo(30);
    expect(p.x).toBeLessThan(200);
  });
});

// "Arrange" must never leave boxes stacked or touching, but should keep the
// user's layout: boxes already far apart do not move.
describe("spreadBoxes", () => {
  const box = (x: number, y: number) => ({ x, y, width: 160, height: 60 });
  const gapBetween = (a: ReturnType<typeof box>, b: ReturnType<typeof box>) =>
    Math.max(
      Math.abs(a.x + 80 - (b.x + 80)) - 160,
      Math.abs(a.y + 30 - (b.y + 30)) - 60,
    );

  it("separates boxes stacked on top of each other", () => {
    const [a, b, c] = spreadBoxes([box(0, 0), box(0, 0), box(10, 5)], 40);
    expect(gapBetween(a, b)).toBeGreaterThanOrEqual(39.9);
    expect(gapBetween(a, c)).toBeGreaterThanOrEqual(39.9);
    expect(gapBetween(b, c)).toBeGreaterThanOrEqual(39.9);
  });

  it("leaves boxes that are already spaced out where they are", () => {
    const input = [box(0, 0), box(400, 0), box(0, 300)];
    expect(spreadBoxes(input, 40)).toEqual(input);
  });

  it("pushes apart boxes that are too close, keeping their order", () => {
    const [a, b] = spreadBoxes([box(0, 0), box(170, 0)], 40);
    expect(b.x - a.x - 160).toBeCloseTo(40);
    expect(a.x).toBeLessThan(b.x);
  });
});
