import { expect, it } from "vitest";

import { canSwipeAway } from "@/features/terminal/swipeAway";

const at = (over: Partial<Parameters<typeof canSwipeAway>[0]>) =>
  canSwipeAway({ full: false, chrome: false, pad: false, ...over });

it("swipes away from the shell, its history being a scroll iOS can see", () => {
  expect(at({})).toBe(true);
  expect(at({ chrome: true })).toBe(true);
});

it("keeps a full-screen program's drags, except from the header or keys", () => {
  expect(at({ full: true })).toBe(false);
  expect(at({ full: true, chrome: true })).toBe(true);
});

it("never swipes away from the arrow pad, where a drag types arrows", () => {
  expect(at({ pad: true })).toBe(false);
  expect(at({ pad: true, chrome: true })).toBe(false);
  expect(at({ pad: true, full: true, chrome: true })).toBe(false);
});
