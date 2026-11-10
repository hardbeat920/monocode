import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";

const STORAGE_KEY = "toolCalls.collapsed";

function load(): boolean {
  try {
    return Storage.getItemSync(STORAGE_KEY) !== "false";
  } catch {
    return true;
  }
}

// Held here, so every turn showing work sees a change made in settings.
let collapsed: boolean | null = null;
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

/**
 * Whether work phases show only their label until tapped, even the one in
 * progress; on until turned off. Off, the live phase stays open on its
 * latest steps.
 */
export function useCollapsedToolCalls() {
  const value = useSyncExternalStore(subscribe, () => (collapsed ??= load()));
  const set = (next: boolean) => {
    collapsed = next;
    void Storage.setItem(STORAGE_KEY, String(next));
    listeners.forEach((listener) => listener());
  };
  return [value, set] as const;
}
