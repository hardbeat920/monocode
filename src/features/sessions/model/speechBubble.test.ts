import { describe, expect, it } from "vitest";
import { drawSpeechBubble } from "./speechBubble";

type Point = [number, number];

/**
 * A recording stand-in for the 2D context, so we can assert on the geometry
 * the bubble actually traces without needing a real canvas.
 */
function recorder() {
  const paths: Point[][] = [];
  let current: Point[] = [];
  const ctx = {
    font: "",
    letterSpacing: "",
    globalAlpha: 1,
    lineJoin: "",
    lineWidth: 0,
    fillStyle: "",
    strokeStyle: "",
    textBaseline: "",
    textAlign: "",
    save() {},
    restore() {},
    measureText: (text: string) => ({ width: text.length * 6.5 }),
    beginPath() {
      current = [];
    },
    moveTo(x: number, y: number) {
      current.push([x, y]);
    },
    lineTo(x: number, y: number) {
      current.push([x, y]);
    },
    closePath() {
      paths.push(current);
    },
    fill() {},
    stroke() {},
    fillText() {},
  };
  return { ctx: ctx as unknown as CanvasRenderingContext2D, paths };
}

const BOUNDS = { width: 700, height: 200 };
const THEME = { fg: "#fff", bg: "#000" };
const HEAD = 6;

/** The shadow is traced first, so the bubble itself is the second path. */
function bubblePath(headX: number, headY: number, text = "HELLO THERE!") {
  const { ctx, paths } = recorder();
  drawSpeechBubble(ctx, text, headX, headY, HEAD, BOUNDS, 1, THEME);
  const path = paths[1];
  if (!path) throw new Error("no bubble was drawn");
  return path;
}

const box = (path: Point[]) => ({
  left: Math.min(...path.map((p) => p[0])),
  right: Math.max(...path.map((p) => p[0])),
  top: Math.min(...path.map((p) => p[1])),
  bottom: Math.max(...path.map((p) => p[1])),
});

describe("speech bubble", () => {
  it("points its tail at the snake's head", () => {
    const headX = 300;
    const path = bubblePath(headX, 90);
    // The tail's tip is the lowest point, and its leading edge sits on the head.
    const bottom = Math.max(...path.map((p) => p[1]));
    const tip = path.filter((p) => p[1] === bottom);
    expect(Math.min(...tip.map((p) => p[0]))).toBe(headX);
  });

  it("sits above the head when there is room", () => {
    const headY = 90;
    expect(box(bubblePath(300, headY)).bottom).toBeLessThanOrEqual(headY);
  });

  it("flips below the head when it would run off the top", () => {
    const headY = 4;
    expect(box(bubblePath(300, headY)).top).toBeGreaterThanOrEqual(headY);
  });

  it("keeps itself on screen at either edge", () => {
    for (const headX of [0, 4, 340, BOUNDS.width - 4, BOUNDS.width]) {
      const { left, right } = box(bubblePath(headX, 90));
      expect(left).toBeGreaterThanOrEqual(0);
      expect(right).toBeLessThanOrEqual(BOUNDS.width);
    }
  });

  it("grows with the text", () => {
    const short = box(bubblePath(300, 90, "YOINK"));
    const long = box(bubblePath(300, 90, "RESOLVING DEPENDENCY"));
    expect(long.right - long.left).toBeGreaterThan(short.right - short.left);
  });

  it("snaps every edge to the pixel grid", () => {
    for (const [x, y] of bubblePath(301, 91)) {
      expect(x % 2).toBe(0);
      expect(y % 2).toBe(0);
    }
  });

  it("draws nothing once it has faded out", () => {
    const { ctx, paths } = recorder();
    drawSpeechBubble(ctx, "HELLO THERE!", 300, 90, HEAD, BOUNDS, 0, THEME);
    expect(paths).toHaveLength(0);
  });
});
