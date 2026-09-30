import { describe, expect, it } from "vitest";
import { hslToRgb } from "./colorUtils";

describe("hslToRgb", () => {
  it("matches the theme backgrounds the native window is filled with", () => {
    expect(hslToRgb(240, 0, 9)).toEqual({ r: 23, g: 23, b: 23 });
    expect(hslToRgb(240, 0, 97)).toEqual({ r: 247, g: 247, b: 247 });
  });

  it("tints toward the accent hue", () => {
    expect(hslToRgb(0, 100, 50)).toEqual({ r: 255, g: 0, b: 0 });
    expect(hslToRgb(120, 100, 50)).toEqual({ r: 0, g: 255, b: 0 });
    expect(hslToRgb(240, 100, 50)).toEqual({ r: 0, g: 0, b: 255 });
  });

  it("wraps hue and clamps the ends", () => {
    expect(hslToRgb(-120, 100, 50)).toEqual(hslToRgb(240, 100, 50));
    expect(hslToRgb(600, 100, 50)).toEqual(hslToRgb(240, 100, 50));
    expect(hslToRgb(240, 0, -20)).toEqual({ r: 0, g: 0, b: 0 });
    expect(hslToRgb(240, 0, 120)).toEqual({ r: 255, g: 255, b: 255 });
  });
});
