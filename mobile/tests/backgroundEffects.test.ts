import { describe, expect, it } from "vitest";
import { effectPixels } from "@/features/background/effects";
import {
  backgroundOpacity,
  EFFECTS,
  normalizeBackground,
} from "@/features/background/settings";

const image = () => {
  const pixels = new Uint8Array(24 * 32 * 4);
  for (let i = 0; i < pixels.length; i += 4)
    pixels.set([80 + (i % 128), 90, 120, 255], i);
  return { width: 24, height: 32, pixels };
};
describe("mobile desktop background parity", () => {
  it("keeps legacy photos, strength and empty-only dither while new preferences use desktop defaults", () => {
    expect(normalizeBackground({ image: "saved.jpg", opacity: 0.45 })).toEqual({
      image: "saved.jpg",
      opacity: 0.45,
      sessionOpacity: 0.24,
      scope: "empty",
      effect: "dither",
    });
    expect(normalizeBackground(null)).toEqual({
      image: null,
      opacity: 0.24,
      sessionOpacity: 0.24,
      scope: "all",
      effect: "none",
    });
  });
  it("validates persisted values without disturbing a valid photo", () => {
    expect(
      normalizeBackground({
        image: "saved.jpg",
        effect: "invalid",
        scope: "invalid",
        opacity: 2,
        sessionOpacity: -1,
      }),
    ).toMatchObject({
      image: "saved.jpg",
      effect: "none",
      scope: "all",
      opacity: 0.65,
      sessionOpacity: 0.05,
    });
    expect(
      normalizeBackground({ image: "../photo.jpg", opacity: NaN }),
    ).toMatchObject({ image: null, opacity: 0.24 });
  });
  it("switches strength on the first message, hides populated sessions in empty-only mode", () => {
    const settings = normalizeBackground({
      image: "photo.jpg",
      effect: "none",
      scope: "all",
      opacity: 0.6,
      sessionOpacity: 0.1,
    });
    expect(backgroundOpacity(settings, true)).toBe(0.6);
    expect(backgroundOpacity(settings, false)).toBe(0.1);
    expect(backgroundOpacity({ ...settings, scope: "empty" }, false)).toBe(0);
    expect(backgroundOpacity({ ...settings, scope: "empty" }, true)).toBe(0.6);
  });
  it("renders every desktop effect as distinct, static artwork without mutating the source", () => {
    const source = image();
    const before = new Uint8Array(source.pixels);
    const outputs = EFFECTS.map((effect) =>
      effectPixels(source, effect, false, true),
    );
    expect(new Set(outputs.map((pixels) => pixels.join(","))).size).toBe(
      EFFECTS.length,
    );
    for (const output of outputs) {
      expect(output.length).toBe(source.pixels.length);
      expect(output[3]).toBeGreaterThanOrEqual(163);
      expect(output[output.length - 1]).toBe(0);
    }
    expect(source.pixels).toEqual(before);
  });
  it.each(["ascii", "halftone", "scanlines"] as const)(
    "adapts %s texture to light backgrounds",
    (effect) => {
      expect(effectPixels(image(), effect, true, true)).not.toEqual(
        effectPixels(image(), effect, false, true),
      );
    },
  );
  it.each(EFFECTS)(
    "uses a separate, quieter populated-session fade for %s",
    (effect) => {
      const source = image();
      expect(effectPixels(source, effect, false, true)).not.toEqual(
        effectPixels(source, effect, false, false),
      );
    },
  );
  it("keeps the original photo colors for None", () => {
    const source = image();
    expect(effectPixels(source, "none", false, true).slice(0, 12)).toEqual(
      source.pixels.slice(0, 12),
    );
  });
});
