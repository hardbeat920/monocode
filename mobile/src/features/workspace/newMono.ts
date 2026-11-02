// The new-Mono sheet is a route of its own, presented over the list or the
// drawer, so the Mono it made comes back to them through here to be opened.
const listeners = new Set<(monoId: string) => void>();

/** Hands the Mono just made to whichever screen opens it. */
export function monoCreated(monoId: string) {
  listeners.forEach((listener) => listener(monoId));
}

/** Calls `listener` with each Mono made; returns the unsubscribe. */
export function onMonoCreated(listener: (monoId: string) => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
