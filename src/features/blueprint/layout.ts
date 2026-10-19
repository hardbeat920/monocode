import dagre from "@dagrejs/dagre";
import type { Rect } from "./geometry";

/**
 * Lays the whole graph out in layers following the arrows, left to right:
 * what calls comes before what it calls. Discards the current positions.
 */
export function autoLayout<T extends Rect & { id: string }>(
  boxes: T[],
  edges: { source: string; target: string }[],
): T[] {
  const g = new dagre.graphlib.Graph();
  g.setGraph({ rankdir: "LR", nodesep: 48, ranksep: 120 });
  g.setDefaultEdgeLabel(() => ({}));
  for (const b of boxes) g.setNode(b.id, { width: b.width, height: b.height });
  for (const e of edges) {
    if (g.hasNode(e.source) && g.hasNode(e.target))
      g.setEdge(e.source, e.target);
  }
  dagre.layout(g);
  // dagre returns centers; boxes are positioned by their top-left corner.
  return boxes.map((b) => {
    const { x, y } = g.node(b.id);
    return { ...b, x: x - b.width / 2, y: y - b.height / 2 };
  });
}
