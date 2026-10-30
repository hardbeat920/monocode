import * as Notifications from "expo-notifications";
import { useEffect, useRef } from "react";

import { useConnection } from "@/features/connection/ConnectionProvider";
import type { SessionSummary } from "@/features/workspace/types";

import {
  activitySessions,
  endActivity,
  pushedActivity,
  rememberLink,
  rememberDesktop,
  showSessions,
  useLiveActivityPreference,
} from "./liveActivity";

/** Redraws the Live Activity from each catalog the open app polls, and
 *  from a push that lands while it is open (the background task only hears
 *  the ones that arrive while it is not). */
export function SessionLiveActivity() {
  const connection = useConnection();
  const [enabled] = useLiveActivityPreference();
  const desktop = connection.pairing?.desktop.name ?? "Desktop";
  // The sessions as last drawn from, to see which just finished; null until
  // a list from the desktop itself arrives.
  const before = useRef<SessionSummary[] | null>(null);
  const shown = useRef<string | null>(null);

  useEffect(() => {
    rememberDesktop(desktop);
  }, [desktop]);

  // Kept current as the desktop's addresses change, for the Approve button.
  const pairing = connection.pairing;
  useEffect(() => {
    rememberLink(enabled ? pairing : null);
  }, [enabled, pairing]);

  useEffect(() => {
    if (!enabled) return;
    const subscription = Notifications.addNotificationReceivedListener(
      (notification) => {
        const sessions = pushedActivity(notification.request.content.data);
        if (sessions) void showSessions(desktop, sessions);
      },
    );
    return () => subscription.remove();
  }, [enabled, desktop]);

  useEffect(() => {
    if (!enabled) {
      before.current = null;
      shown.current = null;
      void endActivity();
      return;
    }
    if (!connection.connected) return;
    const { sessions, projects } = connection.catalog;
    const next = activitySessions(before.current, sessions, projects);
    before.current = sessions;
    const key = JSON.stringify(next);
    if (key === shown.current) return;
    shown.current = key;
    void showSessions(desktop, next);
  }, [enabled, connection.connected, connection.catalog, desktop]);

  return null;
}
