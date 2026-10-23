import * as Notifications from "expo-notifications";
import { router } from "expo-router";
import { useEffect, useRef, useState } from "react";
import { AppState, Platform } from "react-native";

import { useConnection } from "@/features/connection/ConnectionProvider";
import {
  NOTIFICATION_CATEGORIES,
  useNotificationPreferences,
  type NotificationCategory,
} from "@/features/settings/notifications";
import type { SessionSummary } from "@/features/workspace/types";

// Only for when the app is out of view; one landing as it comes back stays
// in the list.
Notifications.setNotificationHandler({
  handleNotification: async () => {
    const away = AppState.currentState !== "active";
    return {
      shouldShowBanner: away,
      shouldShowList: true,
      shouldPlaySound: away,
      shouldSetBadge: false,
    };
  },
});

type Seen = Pick<SessionSummary, "busy" | "waiting">;
type Data = { projectId: string; sessionId: string };
const SESSION_CHANNEL = "sessions";

/**
 * Local notifications for the desktop's sessions, after its own: an agent
 * that finishes, or stops for an approval or a question. They come from the
 * session list as the phone polls it, so only while the app runs, and only
 * while it is out of view. Tapping one opens its session.
 */
export function SessionNotifications() {
  const connection = useConnection();
  const preferences = useNotificationPreferences();
  const current = useRef(preferences);
  // Read by the effects below; kept up to date ahead of them.
  useEffect(() => {
    current.current = preferences;
  });
  // Each session as last seen; null until a list from the desktop itself
  // arrives, as the one cached from last time is no news.
  const seen = useRef<Map<string, Seen> | null>(null);
  const anyOn = NOTIFICATION_CATEGORIES.some(({ id }) =>
    preferences.enabled(id),
  );

  // Ask after pairing, only when alerts are enabled and the OS allows asking.
  useEffect(() => {
    if (!anyOn || !connection.pairing) return;
    const prepare = async () => {
      // Android needs a channel before its permission prompt can appear.
      if (Platform.OS === "android") {
        await Notifications.setNotificationChannelAsync(SESSION_CHANNEL, {
          name: "Session alerts",
          importance: Notifications.AndroidImportance.DEFAULT,
        });
      }
      const settings = await Notifications.getPermissionsAsync();
      const shouldRequest =
        Platform.OS === "ios"
          ? settings.ios?.status ===
            Notifications.IosAuthorizationStatus.NOT_DETERMINED
          : settings.status !== "granted";
      if (shouldRequest && settings.canAskAgain) {
        await Notifications.requestPermissionsAsync();
      }
    };
    void prepare().catch((reason) =>
      console.warn("Could not enable session notifications", reason),
    );
  }, [anyOn, connection.pairing]);

  useEffect(() => {
    if (!connection.connected) {
      seen.current = null;
      return;
    }
    const sessions = connection.catalog.sessions;
    const before = seen.current;
    seen.current = new Map(
      sessions.map((session) => [
        session.id,
        { busy: session.busy, waiting: session.waiting },
      ]),
    );
    if (!before) return;
    // In front of the user, the app itself shows what changed.
    if (AppState.currentState === "active") return;
    const preferences = current.current;
    const projects = connection.catalog.projects;
    for (const session of sessions) {
      const last = before.get(session.id);
      if (!last) continue;
      const category: NotificationCategory | null =
        session.waiting && session.waiting !== last.waiting
          ? "agentInput"
          : last.busy && !session.busy && !session.waiting
            ? "agentFinished"
            : null;
      if (!category || !preferences.enabled(category)) continue;
      const project = projects.find((entry) => entry.id === session.projectId);
      void Notifications.scheduleNotificationAsync({
        content: {
          title: session.title || "Session",
          subtitle: project?.name,
          body:
            category === "agentFinished"
              ? "Finished"
              : session.waiting === "approval"
                ? "Needs your approval"
                : "Has a question for you",
          data: {
            projectId: session.projectId,
            sessionId: session.id,
          } satisfies Data,
        },
        trigger: Platform.OS === "android" ? { channelId: SESSION_CHANNEL } : null,
      });
    }
  }, [connection.connected, connection.catalog]);

  // A tap opens the session, the app's last one too if it launched it.
  const response = Notifications.useLastNotificationResponse();
  useEffect(() => {
    if (response?.actionIdentifier !== Notifications.DEFAULT_ACTION_IDENTIFIER)
      return;
    const data = response.notification.request.content.data as
      Partial<Data> | undefined;
    void Notifications.clearLastNotificationResponseAsync();
    if (!data?.projectId || !data.sessionId) return;
    router.push({
      pathname: "/project/[projectId]/session/[sessionId]",
      params: { projectId: data.projectId, sessionId: data.sessionId },
    });
  }, [response]);

  return null;
}

/** Whether notifications are turned off for the app; checked again each
 *  time the app comes back from system settings. */
export function useNotificationsBlocked() {
  const [blocked, setBlocked] = useState(false);
  useEffect(() => {
    const check = () =>
      void Notifications.getPermissionsAsync().then((settings) =>
        setBlocked(settings.status === "denied" && !settings.canAskAgain),
      );
    check();
    const subscription = AppState.addEventListener("change", (state) => {
      if (state === "active") check();
    });
    return () => subscription.remove();
  }, []);
  return blocked;
}
