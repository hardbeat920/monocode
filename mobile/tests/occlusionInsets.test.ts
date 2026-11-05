import { describe, expect, it } from "vitest";
import { occlusionInsets } from "@/ui/occlusionInsets";

const measure = (regions: { x: number; y: number; width: number; height: number }[]) =>
  occlusionInsets({ supported: true, width: 469, height: 160, regions });

describe("floating control occlusions", () => {
  it("reclaims the rail when there are no overlapping controls", () => {
    expect(measure([])).toEqual({ left: 0, right: 0 });
    expect(measure([{ x: 385, y: -200, width: 84, height: 100 }])).toEqual({ left: 0, right: 0 });
  });
  it("clears a right rail control, including a partial vertical overlap", () => {
    expect(measure([{ x: 385, y: -20, width: 84, height: 40 }])).toEqual({ left: 0, right: 84 });
  });
  it("handles the rail on the left in Split View", () => {
    expect(measure([{ x: -10, y: 20, width: 94, height: 80 }])).toEqual({ left: 84, right: 0 });
  });
  it("clears simultaneous controls on both sides", () => {
    expect(measure([
      { x: 0, y: 10, width: 60, height: 50 },
      { x: 385, y: 0, width: 84, height: 60 },
    ])).toEqual({ left: 60, right: 84 });
  });
  it("ignores touching boundaries and regions outside the control", () => {
    expect(measure([
      { x: 385, y: 160, width: 84, height: 20 },
      { x: 385, y: -20, width: 84, height: 20 },
      { x: 469, y: 0, width: 84, height: 160 },
    ])).toEqual({ left: 0, right: 0 });
  });
  it("uses the largest clearance on each edge as Live Activities expand", () => {
    expect(measure([
      { x: 400, y: 0, width: 69, height: 40 },
      { x: 365, y: 35, width: 104, height: 120 },
    ])).toEqual({ left: 0, right: 104 });
  });
});
