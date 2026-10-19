// The project picker is a route of its own, presented over the project
// screen, so its pick comes back to that screen through here.
const listeners = new Set<(projectId: string) => void>();

/** Hands a pick from the picker to the screen under it. False when no
 *  screen took it, as a mono's doesn't. */
export function pickProject(projectId: string) {
  listeners.forEach((listener) => listener(projectId));
  return listeners.size > 0;
}

/** Calls `listener` with each pick; returns the unsubscribe. */
export function onProjectPick(listener: (projectId: string) => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
