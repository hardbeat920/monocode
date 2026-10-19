let active = 0;
const listeners = new Set<() => void>();

/**
 * Marks a pane slide or resize drag in progress. Width-dependent measurement
 * can wait for it to end and run once, rather than on every frame. Returns
 * the matching end; calling it again does nothing.
 */
export function beginLayoutTransition(): () => void {
  active++;
  let ended = false;
  return () => {
    if (ended) return;
    ended = true;
    active--;
    if (active > 0) return;
    for (const listener of [...listeners]) listener();
  };
}

export function layoutTransitionActive(): boolean {
  return active > 0;
}

/** Called each time the last running transition ends. */
export function onLayoutTransitionEnd(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
