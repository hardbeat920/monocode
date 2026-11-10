import Constants from "expo-constants";
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
/** Further and further apart, for a push token that didn't come. */
const RETRY_MS = [5_000, 15_000, 45_000];
/** Where a pushed notification opens: a session, or a Mono's chat. */
type Target = Partial<Data> & { monoId?: string };
const SESSION_CHANNEL = "sessions";

/**
 * Notifications for the desktop's sessions. A desktop that can sends them
 * by push, so they arrive with the app closed: for sessions the phone
 * started, and whatever a Mono says. The phone registers its push token and
 * the categories it turned off each time it connects.
 *
 * With the older desktop app, they are local, after its own: an agent that
 * finishes, or stops for an approval or a question, from the session list
 * as the phone polls it, so only while the app runs and is out of view.
 * Tapping one opens its session or Mono.
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
  // Android has no push credentials yet (FCM), so it keeps local alerts.
  const push = connection.catalog.push === true && Platform.OS === "ios";
  const disabled = NOTIFICATION_CATEGORIES.filter(
    ({ id }) => !preferences.enabled(id),
  )
    .map(({ id }) => id)
    .join(",");
  // Bumped once the permission prompt is answered, and on coming back from
  // system settings, where notifications may have been turned on or off.
  const [permissionCheck, setPermissionCheck] = useState(0);
  const command = useRef(connection.command);
  useEffect(() => {
    command.current = connection.command;
  });
  // Whether the desktop holds this phone's token; until it does, the phone
  // goes on posting its own alerts.
  const registered = useRef(false);

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
      setPermissionCheck((count) => count + 1);
    };
    void prepare().catch((reason) =>
      console.warn("Could not enable session notifications", reason),
    );
  }, [anyOn, connection.pairing]);

  useEffect(() => {
    if (!push) return;
    const subscription = AppState.addEventListener("change", (state) => {
      if (state === "active") setPermissionCheck((count) => count + 1);
    });
    return () => subscription.remove();
  }, [push]);

  // Tell the desktop where to push, or that it no longer can.
  useEffect(() => {
    if (!push || !connection.connected) return;
    let cancelled = false;
    const register = async () => {
      const settings = await Notifications.getPermissionsAsync();
      const token = settings.granted
        ? (
            await Notifications.getExpoPushTokenAsync({
              projectId: Constants.expoConfig?.extra?.eas?.projectId,
            })
          ).data
        : null;
      if (cancelled) return;
      await command.current("device.push", {
        token,
        disabled: disabled ? disabled.split(",") : [],
      });
      registered.current = token !== null;
    };
    // A token can fail to come, say while offline: try again a few times.
    let timer: ReturnType<typeof setTimeout> | undefined;
    const attempt = (tries: number) =>
      void register().catch((reason) => {
        if (cancelled) return;
        if (tries < RETRY_MS.length) {
          timer = setTimeout(() => attempt(tries + 1), RETRY_MS[tries]);
        } else {
          console.warn("Could not register for push notifications", reason);
        }
      });
    attempt(0);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [push, connection.connected, disabled, permissionCheck]);

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
    // In front of the user, the app itself shows what changed; the desktop
    // that pushes sends the rest.
    if (AppState.currentState === "active" || (push && registered.current))
      return;
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
  }, [connection.connected, connection.catalog, push]);

  // A tap opens the session or Mono, the app's last one too if it launched it.
  const response = Notifications.useLastNotificationResponse();
  useEffect(() => {
    if (response?.actionIdentifier !== Notifications.DEFAULT_ACTION_IDENTIFIER)
      return;
    const data = response.notification.request.content.data as
      Target | undefined;
    void Notifications.clearLastNotificationResponseAsync();
    // A Mono's habit run names its session too: that opens as the sheet a
    // Mono's sessions do, in whichever project, picked for the phone or not.
    if (data?.monoId && data.projectId && data.sessionId) {
      router.push({
        pathname: "/session",
        params: { projectId: data.projectId, sessionId: data.sessionId },
      });
      return;
    }
    if (data?.monoId) {
      router.push({ pathname: "/mono/[monoId]", params: { monoId: data.monoId } });
      return;
    }
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
