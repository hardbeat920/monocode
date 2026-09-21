import { useSyncExternalStore } from "react";
import {
  loadNotificationPreferences,
  notificationPreferencesSnapshot,
  subscribeNotificationPreferences,
} from "../model/notificationPreferences";

/** Share persisted preferences and mute expiry updates across menus and Settings. */
export function useProjectNotificationPreferences() {
  useSyncExternalStore(
    subscribeNotificationPreferences,
    notificationPreferencesSnapshot,
    notificationPreferencesSnapshot,
  );
  return loadNotificationPreferences();
}
