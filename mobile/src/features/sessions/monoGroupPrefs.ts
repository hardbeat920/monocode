import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";

/**
 * How a desktop's pinned Monos group looks on the Sessions pages: shown at
 * all (the filter menu's "Monos"), and open or folded. Kept per desktop, one
 * for all its projects.
 */
export type MonoGroupPrefs = { shown: boolean; collapsed: boolean };

export const DEFAULT_PREFS: MonoGroupPrefs = { shown: true, collapsed: false };

const storageKey = (desktop: string) => `monocode.monoGroup.${desktop}`;
const cache = new Map<string, MonoGroupPrefs>();
const listeners = new Set<() => void>();

export function parsePrefs(raw: string | null | undefined): MonoGroupPrefs {
  try {
    const value = raw ? (JSON.parse(raw) as Partial<MonoGroupPrefs>) : {};
    return {
      shown: typeof value.shown === "boolean" ? value.shown : DEFAULT_PREFS.shown,
      collapsed: typeof value.collapsed === "boolean" ? value.collapsed : DEFAULT_PREFS.collapsed,
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

export function readMonoGroupPrefs(desktop: string | undefined): MonoGroupPrefs {
  if (!desktop) return DEFAULT_PREFS;
  const known = cache.get(desktop);
  if (known) return known;
  let prefs = DEFAULT_PREFS;
  try {
    prefs = parsePrefs(Storage.getItemSync(storageKey(desktop)));
  } catch {
    // Unreadable storage leaves the defaults.
  }
  cache.set(desktop, prefs);
  return prefs;
}

export function setMonoGroupPrefs(desktop: string | undefined, change: Partial<MonoGroupPrefs>) {
  if (!desktop) return;
  const next = { ...readMonoGroupPrefs(desktop), ...change };
  cache.set(desktop, next);
  void Storage.setItem(storageKey(desktop), JSON.stringify(next));
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useMonoGroupPrefs(desktop: string | undefined) {
  return useSyncExternalStore(subscribe, () => readMonoGroupPrefs(desktop));
}

/** Forgets what was read, for tests. */
export function resetMonoGroupPrefs() {
  cache.clear();
}
