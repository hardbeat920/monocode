import { useEffect, useState } from "react";

const KEY = "monocode.inboxSeen";
const LEGACY_KEY = "monocode.inboxSeenAt";

export type InboxSeenEntry = {
  key: string;
  updatedAt: string;
};

type SeenMap = Record<string, number>;

type SeenStore = {
  seeded: boolean;
  items: SeenMap;
};

type Listener = () => void;

const listeners = new Set<Listener>();

export function inboxUpdatedAt(item: { updatedAt: string }): number {
  const value = Date.parse(item.updatedAt);
  return Number.isFinite(value) ? value : 0;
}

function notifyInboxSeen() {
  for (const listener of listeners) listener();
}

function isSeenMap(value: unknown): value is SeenMap {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  return Object.values(value).every(
    (entry) => typeof entry === "number" && Number.isFinite(entry),
  );
}

function loadInboxSeenStore(): SeenStore {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { seeded: false, items: {} };
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return { seeded: false, items: {} };
    }
    const record = parsed as { seeded?: unknown; items?: unknown };
    if (typeof record.seeded === "boolean" && isSeenMap(record.items)) {
      return { seeded: record.seeded, items: record.items };
    }
    if (isSeenMap(parsed)) {
      return { seeded: true, items: parsed };
    }
    return { seeded: false, items: {} };
  } catch {
    return { seeded: false, items: {} };
  }
}

function saveInboxSeenStore(store: SeenStore) {
  try {
    localStorage.setItem(
      KEY,
      JSON.stringify({ seeded: store.seeded, items: store.items }),
    );
    localStorage.removeItem(LEGACY_KEY);
  } catch {
    // private mode / quota
  }
  notifyInboxSeen();
}

function mapFrom(items: readonly InboxSeenEntry[]): SeenMap {
  const next: SeenMap = {};
  for (const item of items) {
    if (!item.key) continue;
    next[item.key] = inboxUpdatedAt(item);
  }
  return next;
}

function mergeSeen(items: SeenMap, entry: InboxSeenEntry): SeenMap {
  if (!entry.key) return items;
  return {
    ...items,
    [entry.key]: Math.max(items[entry.key] ?? 0, inboxUpdatedAt(entry)),
  };
}

function entryIsUnseen(entry: InboxSeenEntry, items: SeenMap): boolean {
  if (!entry.key) return false;
  const was = items[entry.key];
  if (was == null) return true;
  return inboxUpdatedAt(entry) > was;
}

export function inboxSeenIsSeeded(): boolean {
  return loadInboxSeenStore().seeded;
}

export function isInboxEntryUnseen(entry: InboxSeenEntry): boolean {
  const store = loadInboxSeenStore();
  if (!store.seeded) return false;
  return entryIsUnseen(entry, store.items);
}

export function markInboxItemSeen(entry: InboxSeenEntry) {
  const store = loadInboxSeenStore();
  saveInboxSeenStore({
    ...store,
    items: mergeSeen(store.items, entry),
  });
}

export function markInboxItemsSeen(entries: readonly InboxSeenEntry[]) {
  const store = loadInboxSeenStore();
  saveInboxSeenStore({
    ...store,
    items: entries.reduce(mergeSeen, store.items),
  });
}

/** First snapshot of the list is remembered so existing items do not badge. */
export function seedInboxSeenIfNeeded(items: readonly InboxSeenEntry[]) {
  const store = loadInboxSeenStore();
  if (store.seeded || items.length === 0) return;
  saveInboxSeenStore({
    seeded: true,
    items: { ...store.items, ...mapFrom(items) },
  });
}

export function inboxHasUnseenItems(items: readonly InboxSeenEntry[]): boolean {
  const store = loadInboxSeenStore();
  if (!store.seeded) return false;
  return items.some((item) => entryIsUnseen(item, store.items));
}

export function subscribeInboxSeen(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useInboxSeenTick(): number {
  const [tick, setTick] = useState(0);
  useEffect(() => subscribeInboxSeen(() => setTick((value) => value + 1)), []);
  return tick;
}
