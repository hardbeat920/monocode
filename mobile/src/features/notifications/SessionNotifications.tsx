import Constants from "expo-constants";
import * as Notifications from "expo-notifications";
import { router } from "expo-router";
import { useEffect, useRef, useState } from "react";
import { AppState, Platform } from "react-native";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { request } from "@/features/connection/nativeLink";
import {
  liveActivitiesAvailable,
  useLiveActivityPreference,
} from "@/features/liveActivity/liveActivity";
import {
  NOTIFICATION_CATEGORIES,
  useNotificationPreferences,
  type NotificationCategory,
} from "@/features/settings/notifications";
import type { SessionSummary } from "@/features/workspace/types";

// In front of the user, the app itself shows what changed; the alert waits
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
 * Alerts for the desktop's sessions: an agent that finishes, or stops for an
 * approval or a question. The desktop sends them as push notifications, so
 * they arrive with the app closed; this registers the phone with it and opens
 * a session when one is tapped. Android has no push yet, so it raises them
 * itself from the session list as the phone polls it, while the app runs and
 * is out of view.
 */
export function SessionNotifications() {
  const connection = useConnection();
  const preferences = useNotificationPreferences();
  const categories = NOTIFICATION_CATEGORIES.map(({ id }) => id).filter((id) =>
    preferences.enabled(id),
  );
  const anyOn = categories.length > 0;
  const [activityOn] = useLiveActivityPreference();
  // What this desktop last heard, so a poll does not register again.
  const registered = useRef<string | null>(null);
  const current = useRef(preferences);
  // Read by the effects below; kept up to date ahead of them.
  useEffect(() => {
    current.current = preferences;
  });
  // Each session as last seen; null until a list from the desktop itself
  // arrives, as the one cached from last time is no news.
  const seen = useRef<Map<string, Seen> | null>(null);

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

  // Tell the desktop where to send alerts, and which, and whether to wake the
  // app to redraw its Live Activity. Sent again after each reconnect, in case
  // the desktop forgot an expired token.
  const pairing = connection.pairing;
  const wanted = categories.join(",");
  const activity = activityOn && liveActivitiesAvailable();
  useEffect(() => {
    if (!connection.connected) {
      registered.current = null;
      return;
    }
    // Expo's push service reaches Android only through FCM, which this app
    // does not set up yet.
    if (!pairing || Platform.OS !== "ios") return;
    let cancelled = false;
    const register = async () => {
      const settings = await Notifications.getPermissionsAsync();
      // A silent push needs no permission; an alert does.
      const token =
        (wanted && settings.granted) || activity
          ? (
              await Notifications.getExpoPushTokenAsync({
                projectId: Constants.expoConfig?.extra?.eas?.projectId,
              })
            ).data
          : null;
      const kinds = settings.granted && wanted ? wanted.split(",") : [];
      const key = `${pairing.desktop.fingerprint}|${token}|${kinds}|${activity}`;
      if (cancelled || registered.current === key) return;
      await request(
        pairing,
        "device.notify",
        token ? { token, categories: kinds, activity } : {},
      );
      if (!cancelled) registered.current = key;
    };
    void register().catch((reason) =>
      console.warn("Could not register for session notifications", reason),
    );
    return () => {
      cancelled = true;
    };
  }, [connection.connected, pairing, wanted, activity]);

  // Android's alerts until push reaches it: raised here from each new list.
  useEffect(() => {
    if (!connection.connected) {
      seen.current = null;
      return;
    }
    if (Platform.OS !== "android") return;
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
        trigger: { channelId: SESSION_CHANNEL },
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
