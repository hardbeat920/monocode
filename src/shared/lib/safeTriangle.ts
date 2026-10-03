// Submenu "safe triangle": while the pointer travels from a menu row toward
// its open submenu, it may cut across sibling rows. Moves inside the triangle
// between the exit point and the submenu's near edge keep the submenu open.
// https://sanyam.sh/blogs/the-submenu-closes-before-you-get-there

export type Point = { x: number; y: number };
export type Grace = { from: Point; top: Point; bottom: Point };

type Rect = Pick<DOMRect, "left" | "right" | "top" | "bottom">;

export function graceArea(from: Point, submenu: Rect): Grace {
  // The submenu flips sides near the screen edge; aim at whichever edge is closer.
  const x =
    Math.abs(submenu.left - from.x) <= Math.abs(submenu.right - from.x)
      ? submenu.left
      : submenu.right;
  return {
    from,
    top: { x, y: submenu.top },
    bottom: { x, y: submenu.bottom },
  };
}

const side = (a: Point, b: Point, p: Point) =>
  (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);

export function inGrace(p: Point, g: Grace): boolean {
  const a = side(g.from, g.top, p);
  const b = side(g.top, g.bottom, p);
  const c = side(g.bottom, g.from, p);
  return (a >= 0 && b >= 0 && c >= 0) || (a <= 0 && b <= 0 && c <= 0);
}
