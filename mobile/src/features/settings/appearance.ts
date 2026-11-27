import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";
import { Appearance } from "react-native";

/** Light or dark, or whichever the phone is in. */
export type AppearancePreference = "system" | "light" | "dark";

const STORAGE_KEY = "appearance";

function load(): AppearancePreference {
  try {
    const stored = Storage.getItemSync(STORAGE_KEY);
    return stored === "light" || stored === "dark" ? stored : "system";
  } catch {
    return "system";
  }
}

// Held at the window, so native chrome (headers, menus, glass) follows it too,
// and useColorScheme reports it everywhere.
function apply(preference: AppearancePreference) {
  Appearance.setColorScheme(preference === "system" ? "unspecified" : preference);
}

// Held here too, so every screen showing it sees a change made on another.
let current: AppearancePreference = "system";
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

/** Applies the saved preference; called once, before the app first draws. */
export function applySavedAppearance() {
  current = load();
  apply(current);
}

/** The appearance set on this phone, and a setter that saves and applies it. */
export function useAppearancePreference() {
  const preference = useSyncExternalStore(subscribe, () => current);
  const set = (next: AppearancePreference) => {
    current = next;
    apply(next);
    void Storage.setItem(STORAGE_KEY, next);
    listeners.forEach((listener) => listener());
  };
  return [preference, set] as const;
}
