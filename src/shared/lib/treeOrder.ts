/**
 * Ids of visible nodes in depth-first, on-screen order. `children` returns null for
 * collapsed (or not yet loaded) nodes; `include` filters which nodes yield an id
 * (excluded nodes are still descended into). Default include: every node.
 */
export function flattenVisible<T>(
  roots: readonly T[],
  options: {
    children: (node: T) => readonly T[] | null;
    id: (node: T) => string;
    include?: (node: T) => boolean;
  },
): string[] {
  const { children, id, include = () => true } = options;
  const out: string[] = [];
  const visit = (nodes: readonly T[]) => {
    for (const node of nodes) {
      if (include(node)) out.push(id(node));
      const next = children(node);
      if (next) visit(next);
    }
  };
  visit(roots);
  return out;
}
