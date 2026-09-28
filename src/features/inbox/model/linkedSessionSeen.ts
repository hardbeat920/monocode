import type { LinkedWorkItem } from "../../sessions/model/session";
import { linkedWorkItemUpdateKey } from "./linkedSessionUpdates";

const KEY = "monocode.linkedSessionSeen";
const MAX_ENTRIES = 500;

type SeenMap = Record<string, number>;
type Listener = () => void;
type LinkedItemRef = Pick<LinkedWorkItem, "repo" | "kind" | "number">;

const listeners = new Set<Listener>();

function seenKey(sessionId: string, item: LinkedItemRef): string {
  return `${sessionId}::${linkedWorkItemUpdateKey(item)}`;
}

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

export function linkedSessionSeenAt(
  sessionId: string,
  item: LinkedItemRef,
  primaryItem?: LinkedItemRef | null,
): number {
  const seen = loadSeenMap();
  const itemSeen = seen[seenKey(sessionId, item)];
  if (typeof itemSeen === "number") return itemSeen;
  // Pre-multi-link acknowledgements lived under sessionId. Reuse that stamp
  // for the original linkedWorkItem.
  if (
    primaryItem &&
    linkedWorkItemUpdateKey(item) === linkedWorkItemUpdateKey(primaryItem)
  ) {
    return seen[sessionId] ?? 0;
  }
  return 0;
}

/** Remember the exact remote snapshot acknowledged for this session item. */
export function markLinkedSessionUpdateSeen(
  sessionId: string,
  item: LinkedItemRef,
  remoteUpdatedAt: number,
) {
  if (!sessionId || !Number.isFinite(remoteUpdatedAt)) return;
  const current = loadSeenMap();
  const key = seenKey(sessionId, item);
  const next = {
    ...current,
    [key]: Math.max(current[key] ?? 0, remoteUpdatedAt),
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
