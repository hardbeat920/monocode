import { useSyncExternalStore } from "react";
import {
  knownNotificationProjectSelection,
  notificationProjectsSnapshot,
  subscribeNotificationProjects,
} from "../model/notificationProjects";

/** Project paths are already the notification identity; no discovery is needed. */
export function useNotificationProjects(paths: readonly string[]) {
  useSyncExternalStore(
    subscribeNotificationProjects,
    notificationProjectsSnapshot,
    notificationProjectsSnapshot,
  );
  const selection = knownNotificationProjectSelection(paths);
  return {
    projects: selection.projects,
  };
}
