import { subscribeGitChanged } from "../../../platform/tauri/fs";
import {
  sessionCheckpointStatus,
  subscribeReviewChanged,
  type CheckpointFile,
} from "./checkpoint";

type Snapshot = { files: CheckpointFile[]; error: string | null };
type Entry = {
  sessionId: string;
  cwd: string;
  scope?: string;
  snapshot: Snapshot;
  dirty: boolean;
  revision: number;
  listeners: Set<() => void>;
  inFlight?: { revision: number; promise: Promise<void> };
  timer?: number;
};

export const EMPTY_SESSION_REVIEW: Snapshot = { files: [], error: null };
const entries = new Map<string, Entry>();
let unsubscribe: (() => void) | undefined;

function entryFor(sessionId: string, cwd: string): Entry {
  const key = JSON.stringify([sessionId, cwd]);
  let entry = entries.get(key);
  if (!entry) {
    entry = {
      sessionId,
      cwd,
      snapshot: EMPTY_SESSION_REVIEW,
      dirty: true,
      revision: 0,
      listeners: new Set(),
    };
    entries.set(key, entry);
  }
  return entry;
}

function publish(entry: Entry, snapshot: Snapshot) {
  entry.snapshot = snapshot;
  for (const listener of entry.listeners) listener();
}

function invalidate(entry: Entry) {
  entry.revision++;
  entry.dirty = true;
}

function cancelTimer(entry: Entry) {
  if (entry.timer !== undefined) window.clearTimeout(entry.timer);
  entry.timer = undefined;
}

function load(entry: Entry): Promise<void> {
  if (entry.inFlight?.revision === entry.revision)
    return entry.inFlight.promise;
  if (!entry.dirty) return Promise.resolve();
  cancelTimer(entry);
  entry.dirty = false;
  const revision = entry.revision;
  const promise = sessionCheckpointStatus(
    entry.sessionId,
    entry.cwd,
    entry.scope === undefined ? undefined : JSON.parse(entry.scope),
  )
    .then(({ files }) => {
      if (revision === entry.revision) publish(entry, { files, error: null });
    })
    .catch((caught: unknown) => {
      if (revision === entry.revision)
        publish(entry, {
          files: [],
          error: caught instanceof Error ? caught.message : String(caught),
        });
    })
    .finally(() => {
      if (entry.inFlight?.revision === revision) entry.inFlight = undefined;
    });
  entry.inFlight = { revision, promise };
  return promise;
}

function changed(sessionId?: string) {
  for (const entry of entries.values()) {
    if (sessionId && entry.sessionId !== sessionId) continue;
    invalidate(entry);
    cancelTimer(entry);
    if (!entry.listeners.size) continue;
    entry.timer = window.setTimeout(() => {
      entry.timer = undefined;
      if (entry.listeners.size) void load(entry);
    }, 200);
  }
}

function watchChanges() {
  if (unsubscribe) return;
  // Keep invalidations while every view is closed; reopening only reads again
  // if a turn or Git mutation changed the cached result.
  const stopReview = subscribeReviewChanged(changed);
  const stopGit = subscribeGitChanged(() => changed());
  unsubscribe = () => {
    stopReview();
    stopGit();
  };
}

/** Review data lives for this app run, independently of mounted chat panes. */
export const sessionReviewCache = {
  getSnapshot(sessionId: string, cwd: string): Snapshot {
    return entryFor(sessionId, cwd).snapshot;
  },
  subscribe(
    sessionId: string,
    cwd: string,
    scope: string | undefined,
    listener: () => void,
  ) {
    watchChanges();
    const entry = entryFor(sessionId, cwd);
    if (entry.scope !== scope) {
      entry.scope = scope;
      invalidate(entry);
    }
    entry.listeners.add(listener);
    void load(entry);
    return () => {
      entry.listeners.delete(listener);
      if (!entry.listeners.size) cancelTimer(entry);
    };
  },
  refresh(sessionId: string, cwd: string) {
    const entry = entryFor(sessionId, cwd);
    invalidate(entry);
    return load(entry);
  },
  startTurn(sessionId: string, cwd: string) {
    const entry = entryFor(sessionId, cwd);
    invalidate(entry);
    cancelTimer(entry);
    publish(entry, EMPTY_SESSION_REVIEW);
  },
  setFiles(sessionId: string, cwd: string, files: CheckpointFile[]) {
    const entry = entryFor(sessionId, cwd);
    invalidate(entry);
    cancelTimer(entry);
    entry.dirty = false;
    publish(entry, { files, error: null });
  },
};

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    unsubscribe?.();
    for (const entry of entries.values()) {
      cancelTimer(entry);
      invalidate(entry);
    }
  });
}
