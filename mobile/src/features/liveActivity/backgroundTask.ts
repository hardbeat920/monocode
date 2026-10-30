import * as Notifications from "expo-notifications";
import * as TaskManager from "expo-task-manager";
import { Platform } from "react-native";

import {
  liveActivityEnabled,
  pushedActivity,
  rememberedDesktop,
  showSessions,
} from "./liveActivity";

/**
 * The desktop's pushes carry its working sessions (notify::activity). One
 * wakes the app in the background, even unopened since launch, and this
 * redraws the Live Activity from it without reaching the desktop.
 */
const TASK = "monocode.session-activity";

if (Platform.OS === "ios") {
  TaskManager.defineTask<Notifications.NotificationTaskPayload>(
    TASK,
    async ({ data }) => {
      const sessions = pushedActivity(data);
      if (!sessions || !liveActivityEnabled())
        return Notifications.BackgroundNotificationTaskResult.NoData;
      await showSessions(rememberedDesktop(), sessions);
      return Notifications.BackgroundNotificationTaskResult.NewData;
    },
  );
  void Notifications.registerTaskAsync(TASK).catch((reason) =>
    console.warn("Could not listen for session pushes", reason),
  );
}
