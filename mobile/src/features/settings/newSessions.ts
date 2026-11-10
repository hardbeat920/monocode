import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";

const STORAGE_KEY = "newSessions.openAfterStarting";

function load(): boolean {
  try {
    return Storage.getItemSync(STORAGE_KEY) !== "false";
  } catch {
    return true;
  }
}

let openAfterStarting: boolean | null = null;
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

/** Whether this phone opens a newly started session; on until turned off. */
export function useOpenSessionAfterStarting() {
  const value = useSyncExternalStore(
    subscribe,
    () => (openAfterStarting ??= load()),
  );
  const set = (next: boolean) => {
    openAfterStarting = next;
    void Storage.setItem(STORAGE_KEY, String(next));
    listeners.forEach((listener) => listener());
  };
  return [value, set] as const;
}
