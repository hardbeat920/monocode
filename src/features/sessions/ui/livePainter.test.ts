import { describe, expect, it } from "vitest";
import {
  type Drawn,
  LEAD,
  SETTLE_MS,
  ScrollPredictor,
  pictureShift,
} from "./livePainter";

const at = (scrollY: number, scrollX = 0): Drawn => ({
  seq: 1,
  width: 800,
  height: 600,
  scrollX,
  scrollY,
});

describe("scroll prediction", () => {
  it("moves the picture ahead once the wheel has scrolled this page", () => {
    const predict = new ScrollPredictor();
    predict.seen(at(0), 0);
    // Nothing known yet: the first wheel waits for the page.
    expect(predict.wheel(0, 100, 10)).toBeNull();
    // The page itself moved: from now on the picture goes ahead.
    expect(predict.seen(at(40), 40)).toBeNull();
    expect(predict.wheel(0, 60, 50)).toEqual({ x: 0, y: 100 });
    expect(predict.wheel(0, 40, 60)).toEqual({ x: 0, y: 140 });
    // Pictures on the way there keep the target; the last one reaches it.
    expect(predict.seen(at(120), 80)).toEqual({ x: 0, y: 140 });
    expect(predict.seen(at(140), 100)).toBeNull();
  });

  it("never goes above the top or far ahead of the page", () => {
    const predict = new ScrollPredictor();
    predict.learn(at(50));
    predict.seen(at(50), 0);
    expect(predict.wheel(0, -500, 10)).toEqual({ x: 0, y: 0 });
    const far = predict.wheel(0, 5000, 20)!;
    expect(far.y).toBe(50 + 600 * LEAD);
  });

  it("stops going ahead while the wheel keeps going at the page's end", () => {
    const predict = new ScrollPredictor();
    predict.learn(at(900));
    predict.seen(at(900), 0);
    expect(predict.wheel(0, 40, 200)).toEqual({ x: 0, y: 940 });
    expect(predict.wheel(0, 40, 200 + SETTLE_MS - 1)).toEqual({ x: 0, y: 980 });
    // No picture moved for a while: the page is not going anywhere.
    expect(predict.wheel(0, 40, 200 + SETTLE_MS)).toBeNull();
  });

  it("lets a page be that does not follow the wheel", () => {
    const predict = new ScrollPredictor();
    predict.learn(at(300));
    predict.seen(at(300), 0);
    // The pointer is over a list that scrolls of its own, or the page is
    // at its end: the page's own scroll stays.
    expect(predict.wheel(0, 100, 10)).toEqual({ x: 0, y: 400 });
    expect(predict.seen(at(300), 30)).toEqual({ x: 0, y: 400 });
    expect(predict.settle(10 + SETTLE_MS - 1)).toEqual({ x: 0, y: 400 });
    expect(predict.settle(10 + SETTLE_MS)).toBeNull();
    // Until it shows again that the wheel scrolls it.
    expect(predict.wheel(0, 100, 400)).toBeNull();
    predict.seen(at(340), 420);
    expect(predict.wheel(0, 100, 430)).toEqual({ x: 0, y: 440 });
  });

  it("keeps going ahead from the first notch after a scroll that landed", () => {
    const predict = new ScrollPredictor();
    predict.learn(at(100));
    predict.seen(at(100), 0);
    expect(predict.wheel(0, 74.8, 10)).toEqual({ x: 0, y: 174.8 });
    // The browser rounds the scroll to a device pixel.
    expect(predict.seen(at(174), 60)).toBeNull();
    expect(predict.settle(400)).toBeNull();
    expect(predict.wheel(0, -100, 1000)).toEqual({ x: 0, y: 74 });
  });

  it("starts over on another page", () => {
    const predict = new ScrollPredictor();
    predict.learn(at(10));
    predict.seen(at(10), 0);
    expect(predict.wheel(0, 50, 5)).not.toBeNull();
    predict.reset();
    expect(predict.wheel(0, 50, 6)).toBeNull();
  });
});

describe("picture shift", () => {
  it("is in the picture's own pixels and never more than the picture", () => {
    expect(pictureShift(at(100), null, 1.25, 1000, 750)).toEqual([0, 0]);
    expect(pictureShift(at(100), { x: 0, y: 140 }, 1.25, 1000, 750)).toEqual([
      0, 50,
    ]);
    expect(pictureShift(at(100), { x: 0, y: 9000 }, 1, 800, 600)).toEqual([
      0, 600,
    ]);
    expect(pictureShift(at(0, 30), { x: 0, y: 0 }, 2, 1600, 1200)).toEqual([
      -60, 0,
    ]);
  });
});
