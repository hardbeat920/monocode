import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";

const STORAGE_KEY = "layout.monosInDrawer";

function load(): boolean {
  try {
    return Storage.getItemSync(STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

// Held here, so the drawer and every Sessions page see a change made in settings.
let inDrawer: boolean | null = null;
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

/**
 * Whether the Monos sit in the drawer, above the projects, rather than pinned
 * above each project's sessions. Off until turned on.
 */
export function useMonosInDrawer() {
  const value = useSyncExternalStore(subscribe, () => (inDrawer ??= load()));
  const set = (next: boolean) => {
    inDrawer = next;
    void Storage.setItem(STORAGE_KEY, String(next));
    listeners.forEach((listener) => listener());
  };
  return [value, set] as const;
}

/** Forgets what was read, for tests. */
export function resetMonosInDrawer() {
  inDrawer = null;
}
