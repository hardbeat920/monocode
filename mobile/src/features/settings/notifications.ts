import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";

/**
 * What this phone is notified of: the desktop's agent categories
 * (NOTIFICATION_CATEGORIES in notificationPreferences.ts), each on until
 * turned off.
 */
export const NOTIFICATION_CATEGORIES = [
  { id: "agentInput", label: "Approvals and questions" },
  { id: "agentFinished", label: "Agent finished" },
] as const;

export type NotificationCategory =
  (typeof NOTIFICATION_CATEGORIES)[number]["id"];

const STORAGE_KEY = "notifications.disabled";

function load(): readonly NotificationCategory[] {
  try {
    const parsed: unknown = JSON.parse(
      Storage.getItemSync(STORAGE_KEY) ?? "[]",
    );
    return NOTIFICATION_CATEGORIES.map(({ id }) => id).filter(
      (id) => Array.isArray(parsed) && parsed.includes(id),
    );
  } catch {
    return [];
  }
}

// Held here, so every screen showing them sees a change made on another.
let disabled: readonly NotificationCategory[] | null = null;
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

/** Whether each category is on, and a setter that saves one. */
export function useNotificationPreferences() {
  const off = useSyncExternalStore(subscribe, () => (disabled ??= load()));
  const enabled = (category: NotificationCategory) => !off.includes(category);
  const setEnabled = (category: NotificationCategory, on: boolean) => {
    disabled = on
      ? off.filter((id) => id !== category)
      : [...off.filter((id) => id !== category), category];
    void Storage.setItem(STORAGE_KEY, JSON.stringify(disabled));
    listeners.forEach((listener) => listener());
  };
  return { enabled, setEnabled };
}
