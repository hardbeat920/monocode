const KEY = "monocode.linkedSessionSeen";
const MAX_ENTRIES = 500;

type SeenMap = Record<string, number>;
type Listener = () => void;

const listeners = new Set<Listener>();

function loadSeenMap(): SeenMap {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {};
    }
    const entries = Object.entries(parsed).filter(
      (entry): entry is [string, number] =>
        Boolean(entry[0]) &&
        typeof entry[1] === "number" &&
        Number.isFinite(entry[1]),
    );
    return Object.fromEntries(entries);
  } catch {
    return {};
  }
}

function saveSeenMap(items: SeenMap) {
  try {
    localStorage.setItem(KEY, JSON.stringify(items));
  } catch {
    // Private mode / quota. The in-memory notification still clears this run.
  }
  for (const listener of listeners) listener();
}

export function linkedSessionSeenAt(sessionId: string): number {
  return loadSeenMap()[sessionId] ?? 0;
}

/** Remember the exact remote snapshot acknowledged for this session. */
export function markLinkedSessionUpdateSeen(
  sessionId: string,
  remoteUpdatedAt: number,
) {
  if (!sessionId || !Number.isFinite(remoteUpdatedAt)) return;
  const current = loadSeenMap();
  const next = {
    ...current,
    [sessionId]: Math.max(current[sessionId] ?? 0, remoteUpdatedAt),
  };
  const trimmed = Object.fromEntries(
    Object.entries(next)
      .sort((left, right) => right[1] - left[1])
      .slice(0, MAX_ENTRIES),
  );
  saveSeenMap(trimmed);
}

export function subscribeLinkedSessionSeen(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
