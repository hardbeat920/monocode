import { describe, expect, it } from "vitest";
import { turnsBefore, watchUpdate, watchWindow } from "./mobileWatch";
import type { MobileTurn } from "./mobileTranscript";

const turn = (id: string, text: string): MobileTurn => ({
  id,
  items: [{ type: "text", id: `${id}:a`, text }],
});

describe("mobile watch frames", () => {
  it("sends everything first, then only the turn that changed", () => {
    const first = watchUpdate(null, {
      busy: true,
      older: false,
      turns: [turn("a", "done"), turn("b", "Wor")],
    })!;
    expect(first.update).toMatchObject({ order: ["a", "b"], turns: { length: 2 } });

    const next = watchUpdate(first.baseline, {
      busy: true,
      older: false,
      turns: [turn("a", "done"), turn("b", "Working")],
    })!;
    expect(next.update).toEqual({
      busy: true,
      older: false,
      order: ["a", "b"],
      turns: [turn("b", "Working")],
    });

    expect(
      watchUpdate(next.baseline, {
        busy: true,
        older: false,
        turns: [turn("a", "done"), turn("b", "Working")],
      }),
    ).toBeNull();
    expect(
      watchUpdate(next.baseline, {
        busy: false,
        older: false,
        turns: [turn("a", "done"), turn("b", "Working")],
      })?.update,
    ).toEqual({ busy: false, older: false, order: ["a", "b"], turns: [] });
  });

  it("opens on the latest turns and grows from there", () => {
    const ids = ["a", "b", "c", "d", "e"];
    const opened = watchWindow(ids, null, 2);
    expect(opened).toEqual({ start: 3, from: "d", older: true });
    expect(watchWindow([...ids, "f"], opened.from, 2).start).toBe(3);
    expect(watchWindow(ids, "gone", 10)).toEqual({ start: 0, from: "a", older: false });
  });

  it("pages in the turns before the window", () => {
    const ids = ["a", "b", "c", "d", "e"];
    expect(turnsBefore(ids, "d", 2)).toEqual({ start: 1, end: 3, older: true });
    expect(turnsBefore(ids, "b", 5)).toEqual({ start: 0, end: 1, older: false });
    expect(() => turnsBefore(ids, "gone", 2)).toThrow();
  });
});
