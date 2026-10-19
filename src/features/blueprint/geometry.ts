import type { BlueprintShape } from "./blueprint";

export type Rect = { x: number; y: number; width: number; height: number };
export type Point = { x: number; y: number };
export type Side = "top" | "right" | "bottom" | "left";

/** Center point of a box. */
export function center(r: Rect): Point {
  return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
}

/**
 * Where the line from the box's center toward `toward` crosses its border,
 * pulled `inset` px inside it. Stadiums follow their rounded ends.
 */
export function borderPoint(
  r: Rect,
  shape: BlueprintShape,
  toward: Point,
  inset = 0,
): Point {
  const c = center(r);
  const hw = Math.max(r.width / 2 - inset, 0);
  const hh = Math.max(r.height / 2 - inset, 0);
  const dx = toward.x - c.x;
  const dy = toward.y - c.y;
  if (dx === 0 && dy === 0) return { x: c.x + hw, y: c.y };

  // Rectangle: scale the direction until it touches the nearer side.
  const t = 1 / Math.max(Math.abs(dx) / hw || 0, Math.abs(dy) / hh || 0);
  const p = { x: c.x + dx * t, y: c.y + dy * t };
  if (shape !== "stadium") return p;

  // Stadium: the ends are half-circles of radius hh. Past the straight
  // section, intersect with the end circle instead.
  const straight = Math.max(hw - hh, 0);
  if (Math.abs(p.x - c.x) <= straight) return p;
  const ex = c.x + Math.sign(dx) * straight;
  // Solve |c + s·d − e|² = hh² for the outer root s.
  const ox = c.x - ex;
  const a = dx * dx + dy * dy;
  const b = 2 * (ox * dx);
  const k = ox * ox - hh * hh;
  const s = (-b + Math.sqrt(b * b - 4 * a * k)) / (2 * a);
  return { x: c.x + dx * s, y: c.y + dy * s };
}

/** Which side of the box a border point is on, for curving the arrow. */
export function sideOf(r: Rect, p: Point): Side {
  const c = center(r);
  const nx = (p.x - c.x) / (r.width / 2 || 1);
  const ny = (p.y - c.y) / (r.height / 2 || 1);
  if (Math.abs(nx) >= Math.abs(ny)) return nx >= 0 ? "right" : "left";
  return ny >= 0 ? "bottom" : "top";
}

/**
 * Pushes overlapping or too-close boxes apart until every pair is at least
 * `gap` apart, keeping the drawing's overall arrangement. Each overlap is
 * resolved along the axis that needs the smaller move, split between the two
 * boxes.
 */
export function spreadBoxes<T extends Rect>(boxes: T[], gap = 48): T[] {
  const out = boxes.map((b) => ({ ...b }));
  // ponytail: O(n²) per pass, fine for hand-drawn blueprints (tens of boxes).
  for (let pass = 0; pass < 200; pass++) {
    let moved = false;
    for (let i = 0; i < out.length; i++) {
      for (let j = i + 1; j < out.length; j++) {
        const a = out[i];
        const b = out[j];
        const ca = center(a);
        const cb = center(b);
        const dx = cb.x - ca.x;
        const dy = cb.y - ca.y;
        const needX = (a.width + b.width) / 2 + gap - Math.abs(dx);
        const needY = (a.height + b.height) / 2 + gap - Math.abs(dy);
        if (needX <= 0 || needY <= 0) continue;
        moved = true;
        // Same center: no direction to push, so pick one deterministically.
        if (needX <= needY) {
          const s = (dx === 0 ? (i < j ? 1 : -1) : Math.sign(dx)) * (needX / 2);
          a.x -= s;
          b.x += s;
        } else {
          const s = (dy === 0 ? 1 : Math.sign(dy)) * (needY / 2);
          a.y -= s;
          b.y += s;
        }
      }
    }
    if (!moved) break;
  }
  return out;
}
