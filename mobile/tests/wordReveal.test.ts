import { describe, expect, it } from "vitest";

import { numberWords, parseMarkdown } from "@/features/sessions/markdownParser";
import {
  cadenceFadeMs,
  closeOpenMarkers,
  fadeEase,
  nextCadence,
  revealEnd,
  WordClock,
} from "@/features/sessions/wordReveal";

describe("streamed text reveal", () => {
  it("holds back a word still being written, as on desktop", () => {
    expect(revealEnd("Hello wor", 9, true)).toBe(6);
    expect(revealEnd("Hello wor", 9, false)).toBe(9);
    expect(revealEnd("Hello world ", 12, true)).toBe(12);
  });

  it("fades faster for a fast stream and slower for a slow one", () => {
    expect(cadenceFadeMs(20)).toBe(120);
    expect(cadenceFadeMs(100)).toBe(300);
    expect(cadenceFadeMs(5000)).toBe(400);
    // One long stall only moves the average part of the way.
    expect(nextCadence(160, 60_000)).toBe(160 * 0.7 + 1000 * 0.3);
  });

  it("eases in on the desktop's curve", () => {
    expect(fadeEase(0)).toBeCloseTo(0);
    expect(fadeEase(1)).toBeCloseTo(1);
    expect(fadeEase(0.5)).toBeGreaterThan(0.5);
  });

  it("closes emphasis and code a stream has left open", () => {
    expect(closeOpenMarkers("Fixed the **bu")).toBe("Fixed the **bu**");
    expect(closeOpenMarkers("Run `npm te")).toBe("Run `npm te`");
    expect(closeOpenMarkers("```ts\nconst a = **")).toBe("```ts\nconst a = **");
    expect(closeOpenMarkers("all **done**")).toBe("all **done**");
  });
});

describe("word fading", () => {
  it("numbers words in reading order, leaving code and links whole", () => {
    const blocks = parseMarkdown("One **two** `x` [link](https://a.dev)\n\n- three four");
    const { count, ranges } = numberWords(blocks);
    expect(count).toBe(4);
    expect(ranges).toEqual([
      [0, 2],
      [2, 4],
    ]);
  });

  it("leaves words there on open alone and fades the ones after", () => {
    const clock = new WordClock();
    expect(clock.stamp(3, 1000, false)).toBe(false);
    expect(clock.fading(1000, 200)).toBe(false);
    expect(clock.stamp(5, 2000, false)).toBe(true);
    expect(clock.firstFading(2050, 200)).toBe(3);
    expect(clock.alpha(1, 2050, 200)).toBe(1);
    expect(clock.alpha(4, 2000, 200)).toBe(0);
    expect(clock.alpha(4, 2100, 200)).toBeGreaterThan(0);
    expect(clock.fading(2300, 200)).toBe(false);
  });

  it("fades a reply that arrives whole when asked to", () => {
    const clock = new WordClock();
    expect(clock.stamp(4, 0, true)).toBe(true);
    expect(clock.firstFading(10, 200)).toBe(0);
  });
});
