import { useEffect, useState, useSyncExternalStore } from "react";
import { subscribeGitChanged } from "../lib/fs";
import { looksLikeProject } from "../lib/recents";
import { pathKey } from "../lib/paths";
import {
  knownNotificationProjectSelection,
  loadNotificationProjects,
  notificationProjectsSnapshot,
  refreshNotificationProjects,
  resolveNotificationProject,
  subscribeNotificationProjects,
} from "../lib/notificationProjects";

const watchedPaths = new Map<string, { path: string; subscribers: number }>();
let stopWatching: (() => void) | undefined;

/** One Git/focus listener serves all mounted views, without duplicate refreshes. */
function watchProjects(paths: readonly string[]): () => void {
  const unique = new Map(paths.map((path) => [pathKey(path), path]));
  for (const [key, path] of unique) {
    const entry = watchedPaths.get(key);
    if (entry) entry.subscribers++;
    else watchedPaths.set(key, { path, subscribers: 1 });
  }
  if (watchedPaths.size && !stopWatching) {
    const activePaths = () =>
      [...watchedPaths.values()].map((entry) => entry.path);
    const onResume = () => {
      if (document.hidden) return;
      for (const path of activePaths()) {
        void resolveNotificationProject(path).catch(() => {});
      }
    };
    const unsubscribeGit = subscribeGitChanged(() => {
      void refreshNotificationProjects(activePaths()).catch(() => {});
    });
    window.addEventListener("focus", onResume);
    document.addEventListener("visibilitychange", onResume);
    stopWatching = () => {
      unsubscribeGit();
      window.removeEventListener("focus", onResume);
      document.removeEventListener("visibilitychange", onResume);
    };
  }
  return () => {
    for (const key of unique.keys()) {
      const entry = watchedPaths.get(key);
      if (entry && --entry.subscribers === 0) watchedPaths.delete(key);
    }
    if (!watchedPaths.size) {
      stopWatching?.();
      stopWatching = undefined;
    }
  };
}

/** Read known identities synchronously; discovery and refresh never clear them. */
export function useNotificationProjects(paths: readonly string[]) {
  useSyncExternalStore(
    subscribeNotificationProjects,
    notificationProjectsSnapshot,
    notificationProjectsSnapshot,
  );
  const pathsKey = JSON.stringify([...new Set(paths.filter(looksLikeProject))]);
  const [failure, setFailure] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    const requested = JSON.parse(pathsKey) as string[];
    setFailure(null);
    const discover = () => {
      void Promise.all(requested.map(resolveNotificationProject)).then(
        () => {
          if (active) setFailure(null);
        },
        () => {
          if (active) setFailure(pathsKey);
        },
      );
    };
    const unwatch = watchProjects(requested);
    discover();
    return () => {
      active = false;
      unwatch();
    };
  }, [pathsKey, attempt]);
  const selection = knownNotificationProjectSelection(JSON.parse(pathsKey));
  return {
    projects: loadNotificationProjects(),
    selection,
    loading: selection === null && failure !== pathsKey,
    error:
      selection === null && failure === pathsKey
        ? "Could not load projects. Please try again."
        : null,
    retry: () => setAttempt((value) => value + 1),
  };
}
