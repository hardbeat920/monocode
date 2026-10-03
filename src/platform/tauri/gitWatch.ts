import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { invokeWorkspace, REMOTE_PATH_PREFIX } from "./fs";

type Change = { id: string; failed: boolean };
type Subscription = {
  listeners: Set<() => void>;
  disposed: boolean;
  stop?: () => void;
};
const subscriptions = new Map<string, Subscription>();

/** Share one native watcher per displayed repository. Normal operation has no
 * timer or Git reads until a filesystem notification arrives. */
export function watchGitChanges(cwd: string, listener: () => void): () => void {
  let entry = subscriptions.get(cwd);
  if (!entry) {
    entry = { listeners: new Set(), disposed: false };
    subscriptions.set(cwd, entry);
    // Defer setup until the first listener has been added.
    void Promise.resolve().then(() => start(cwd, entry!));
  }
  entry.listeners.add(listener);
  const subscription = entry;
  return () => {
    subscription.listeners.delete(listener);
    if (subscription.listeners.size || subscription.disposed) return;
    subscription.disposed = true;
    subscriptions.delete(cwd);
    subscription.stop?.();
  };
}

function changed(entry: Subscription) {
  if (!entry.disposed) for (const listener of [...entry.listeners]) listener();
}

function fallback(entry: Subscription) {
  if (entry.disposed) return;
  // Network filesystems or older remote hosts may not support notifications.
  // Keep their existing updates working, without polling supported projects.
  const timer = window.setInterval(() => changed(entry), 2000);
  entry.stop = () => window.clearInterval(timer);
  changed(entry);
}

async function start(cwd: string, entry: Subscription) {
  if (entry.disposed) return;
  const id = crypto.randomUUID();
  if (cwd.startsWith(REMOTE_PATH_PREFIX)) {
    await startRemote(cwd, id, entry);
    return;
  }
  let unlisten: (() => void) | undefined;
  let starting = true;
  let failed = false;
  const stop = () => {
    unlisten?.();
    unlisten = undefined;
    // If setup is still in flight, its completion performs the cleanup.
    if (!starting) void invoke("unwatch_git_changes", { id }).catch(() => {});
  };
  entry.stop = stop;
  try {
    unlisten = await listen<Change>("git-watch-changed", ({ payload }) => {
      if (payload.id !== id || entry.disposed || failed) return;
      if (payload.failed) {
        failed = true;
        stop();
        fallback(entry);
      } else {
        changed(entry);
      }
    });
    if (entry.disposed) {
      unlisten();
      return;
    }
    await invoke("watch_git_changes", { cwd, id });
    starting = false;
    if (entry.disposed || failed) stop();
    else changed(entry); // Covers writes between the initial read and setup.
  } catch {
    starting = false;
    stop();
    if (!failed) fallback(entry);
  }
}

async function startRemote(cwd: string, id: string, entry: Subscription) {
  let started = false;
  const stop = () => {
    if (started)
      void invokeWorkspace("unwatch_git_changes", { cwd, id }).catch(() => {});
  };
  entry.stop = stop;
  try {
    await invokeWorkspace("watch_git_changes", { cwd, id });
    started = true;
    if (entry.disposed) {
      stop();
      return;
    }
    changed(entry);
    while (!entry.disposed) {
      // The host holds this request until a filesystem event arrives. A
      // timeout only renews the connection; it does not run a Git query.
      if (await invokeWorkspace<boolean>("wait_git_changes", { cwd, id }))
        changed(entry);
    }
  } catch {
    stop();
    fallback(entry);
  }
}
