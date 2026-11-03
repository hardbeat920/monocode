import { unzlibSync } from "fflate";
import { describe, expect, it } from "vitest";

import { ditherPixels, encodePng, fadeOut } from "@/features/background/dither";

/** A width × height image of one colour, as RGBA. */
function solid(width: number, height: number, rgb: [number, number, number]) {
  const pixels = new Uint8Array(width * height * 4);
  for (let i = 0; i < pixels.length; i += 4) pixels.set([...rgb, 255], i);
  return { width, height, pixels };
}

describe("background dither", () => {
  it("dithers in 2×2 blocks between full-brightness colour and near black", () => {
    const pixels = ditherPixels(solid(8, 8, [128, 64, 32]));
    const colours = new Set<string>();
    for (let i = 0; i < pixels.length; i += 4) colours.add(pixels.slice(i, i + 3).join(","));
    // Brightened to its peak, or dimmed to 8%, as the desktop does.
    expect([...colours].sort()).toEqual(["10,5,3", "255,128,64"]);
    // Each block is one colour.
    expect(pixels.slice(0, 4)).toEqual(pixels.slice(4, 8));
    expect(pixels.slice(0, 4)).toEqual(pixels.slice(8 * 4, 8 * 4 + 4));
  });

  it("encodes an RGBA PNG of the image", () => {
    const rgba = new Uint8Array([255, 0, 0, 255, 0, 255, 0, 128]);
    const png = encodePng(2, 1, rgba);
    expect([...png.slice(0, 8)]).toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
    const view = new DataView(png.buffer);
    expect([view.getUint32(16), view.getUint32(20)]).toEqual([2, 1]);
    const idatLength = view.getUint32(33);
    const rows = unzlibSync(png.slice(41, 41 + idatLength));
    expect([...rows]).toEqual([0, 255, 0, 0, 255, 0, 255, 0, 128]);
  });

  it("bakes the fade into alpha: opaque at the top, clear by 90% down", () => {
    const faded = fadeOut(solid(1, 10, [255, 255, 255]));
    const row = (y: number) => [...faded.slice(y * 4, y * 4 + 4)];
    // Unmasked and under no overlay at the top.
    expect(row(0)).toEqual([255, 255, 255, 255]);
    // The colour kept, fading out toward the bottom.
    for (let y = 1; y < 10; y += 1) {
      expect(row(y).slice(0, 3)).toEqual([255, 255, 255]);
      expect(row(y)[3]).toBeLessThanOrEqual(row(y - 1)[3]);
    }
    // Past 90%, only the screen behind it.
    expect(row(9)[3]).toBe(0);
  });
});
