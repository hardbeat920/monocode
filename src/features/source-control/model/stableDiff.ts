/**
 * Refresh helpers that keep unchanged diff data referentially stable, so a
 * live-updating review only re-renders (and re-highlights) files that changed.
 */

/** Returns `previous` when `next` has the same own fields by identity. */
export function reuseIfShallowEqual<T extends object>(
  previous: T | undefined,
  next: T,
): T {
  if (!previous) return next;
  const previousKeys = Object.keys(previous) as (keyof T)[];
  const nextKeys = Object.keys(next) as (keyof T)[];
  if (previousKeys.length !== nextKeys.length) return next;
  for (const key of nextKeys) {
    if (!Object.is(previous[key], next[key])) return next;
  }
  return previous;
}

/** Keeps each unchanged item (matched by id) and the whole array when possible. */
export function reuseUnchangedById<T extends { id: string }>(
  previous: readonly T[],
  next: T[],
): T[] {
  const byId = new Map(previous.map((item) => [item.id, item]));
  let same = previous.length === next.length;
  const merged = next.map((item, index) => {
    const kept = reuseIfShallowEqual(byId.get(item.id), item);
    if (kept !== previous[index]) same = false;
    return kept;
  });
  return same ? (previous as T[]) : merged;
}

/** Drops map entries whose keys are gone, keeping the map itself when none are. */
export function pruneMap<K, V>(
  map: Map<K, V>,
  keep: ReadonlySet<K>,
): Map<K, V> {
  let removed = false;
  for (const key of map.keys()) {
    if (!keep.has(key)) {
      removed = true;
      break;
    }
  }
  if (!removed) return map;
  const next = new Map<K, V>();
  for (const [key, value] of map) {
    if (keep.has(key)) next.set(key, value);
  }
  return next;
}
