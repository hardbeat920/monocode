import type { Catalog } from "./ConnectionProvider";
import { deepEqual } from "./deepEqual";

function sameItems<T extends { id: string }>(previous: T[], next: T[]): T[] {
  const known = new Map(previous.map((item) => [item.id, item]));
  const merged = next.map((item) => {
    const before = known.get(item.id);
    return before && deepEqual(before, item) ? before : item;
  });
  return merged.length === previous.length && merged.every((item, i) => item === previous[i])
    ? previous
    : merged;
}

/**
 * The new catalog, keeping the projects, monos and sessions that did not
 * change as they were so rows memoized on them skip re-rendering, and the
 * whole catalog when nothing did so nothing re-renders at all.
 */
export function mergeCatalog(previous: Catalog, next: Catalog): Catalog {
  const projects = sameItems(previous.projects, next.projects);
  const monos = sameItems(previous.monos, next.monos);
  const sessions = sameItems(previous.sessions, next.sessions);
  if (
    projects === previous.projects &&
    monos === previous.monos &&
    sessions === previous.sessions &&
    previous.monoTools === next.monoTools &&
    previous.push === next.push &&
    previous.monoFiles === next.monoFiles
  )
    return previous;
  return { ...next, projects, monos, sessions };
}
