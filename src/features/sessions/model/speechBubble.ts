/**
 * A pixel-art speech bubble drawn straight onto the grid canvas. Every edge is
 * snapped to a chunky unit and the corners and tail are stepped rather than
 * curved, so it reads as sprite art next to the square grid instead of a
 * smooth UI tooltip.
 */

/** One "pixel" of the bubble art, in CSS px. */
const UNIT = 2;
/** Corner chamfer and tail step, both one unit. */
const STEP = UNIT;
const TAIL_STEPS = 3;
const TAIL_H = TAIL_STEPS * STEP;
const SHADOW_OFFSET = 2 * UNIT;

const FONT_PX = 10;
const PAD_X = 3 * UNIT;
const PAD_Y = 2 * UNIT;
const FONT = `600 ${FONT_PX}px ui-monospace, SFMono-Regular, Menlo, monospace`;

const snap = (value: number) => Math.round(value / UNIT) * UNIT;

const clamp = (value: number, min: number, max: number) =>
  Math.max(min, Math.min(max, value));

type Point = [number, number];

/**
 * Outlines the bubble clockwise from the top-left chamfer. The tail is a
 * staircase hanging off whichever edge faces the snake, with its tip at
 * `x + tailX` so it can be aimed at the head.
 */
function outline(
  x: number,
  y: number,
  w: number,
  h: number,
  tailX: number,
  below: boolean,
): Point[] {
  const points: Point[] = [];
  const at = (px: number, py: number) => points.push([px, py]);

  at(x + STEP, y);
  if (below) {
    // Bubble sits under the head, so the tail points up out of the top edge.
    at(x + tailX, y);
    at(x + tailX, y - TAIL_H);
    for (let step = TAIL_STEPS; step > 0; step--) {
      at(x + tailX + (TAIL_STEPS - step + 1) * STEP, y - step * STEP);
      at(x + tailX + (TAIL_STEPS - step + 1) * STEP, y - (step - 1) * STEP);
    }
  }
  at(x + w - STEP, y);
  at(x + w - STEP, y + STEP);
  at(x + w, y + STEP);

  at(x + w, y + h - STEP);
  at(x + w - STEP, y + h - STEP);
  at(x + w - STEP, y + h);
  if (!below) {
    // Tail hangs off the bottom edge, stepping down towards the head.
    for (let step = 1; step <= TAIL_STEPS; step++) {
      at(x + tailX + (TAIL_STEPS - step + 1) * STEP, y + h + (step - 1) * STEP);
      at(x + tailX + (TAIL_STEPS - step + 1) * STEP, y + h + step * STEP);
    }
    at(x + tailX, y + h + TAIL_H);
    at(x + tailX, y + h);
  }
  at(x + STEP, y + h);
  at(x + STEP, y + h - STEP);
  at(x, y + h - STEP);

  at(x, y + STEP);
  at(x + STEP, y + STEP);

  return points;
}

function trace(ctx: CanvasRenderingContext2D, points: Point[]) {
  ctx.beginPath();
  points.forEach(([x, y], index) => {
    if (index === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  });
  ctx.closePath();
}

export type BubbleTheme = { fg: string; bg: string };

/**
 * Draws `text` in a bubble whose tail points at the cell at `headX`/`headY`
 * (already in canvas px). Flips below the head when there's no room above.
 */
export function drawSpeechBubble(
  ctx: CanvasRenderingContext2D,
  text: string,
  headX: number,
  headY: number,
  headSize: number,
  bounds: { width: number; height: number },
  alpha: number,
  theme: BubbleTheme,
) {
  if (alpha <= 0.01) return;

  ctx.save();
  ctx.font = FONT;
  if ("letterSpacing" in ctx) ctx.letterSpacing = "1px";

  const w = snap(ctx.measureText(text).width + PAD_X * 2);
  const h = snap(FONT_PX + PAD_Y * 2);
  const margin = UNIT + SHADOW_OFFSET;

  const above = headY - UNIT - TAIL_H - h;
  const below = above < margin;
  // Snap the whole bubble to the art grid — the head position is eased, so it
  // arrives fractional and the chunky edges would land off-pixel.
  const y = snap(below ? headY + headSize + UNIT + TAIL_H : above);

  // Aim the tail at the head, then keep the whole bubble on screen.
  const preferred = headX - 4 * STEP;
  const x = snap(
    clamp(preferred, margin, Math.max(margin, bounds.width - w - margin)),
  );
  const tailX = clamp(
    snap(headX - x),
    2 * STEP,
    Math.max(2 * STEP, w - (TAIL_STEPS + 2) * STEP),
  );

  ctx.globalAlpha = alpha;
  ctx.lineJoin = "miter";
  ctx.lineWidth = UNIT;

  // Hard offset shadow, the way sprite art fakes depth.
  ctx.fillStyle = theme.fg;
  ctx.globalAlpha = alpha * 0.2;
  trace(ctx, outline(x + SHADOW_OFFSET, y + SHADOW_OFFSET, w, h, tailX, below));
  ctx.fill();

  ctx.globalAlpha = alpha;
  trace(ctx, outline(x, y, w, h, tailX, below));
  ctx.fillStyle = theme.bg;
  ctx.fill();
  ctx.strokeStyle = theme.fg;
  ctx.stroke();

  ctx.fillStyle = theme.fg;
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";
  ctx.fillText(text, x + PAD_X, y + h / 2 + 1);

  ctx.restore();
}
