import type { GithubWorkItem, InboxItem, InboxProvider } from "./githubTasks";
import { inboxNotificationProject } from "../../notifications/model/notificationProjects";

export type InboxTransitionKind =
  "reopened" | "closed" | "merged" | "ready_for_review" | "labeled";

export type InboxTransition = {
  item: InboxItem;
  transition: InboxTransitionKind;
  /** The label that was added, for `labeled`. */
  label?: string;
};

export type InboxTransitionObservation = {
  transitions: InboxTransition[];
  /** Open items that left the list; only a lookup can tell closed from hidden. */
  missing: InboxItem[];
};

const MAX_FAILED_LOOKUPS = 3;
const MAX_LOOKUPS_PER_POLL = 10;

type Snapshot = { item: InboxItem; listed: boolean; failedLookups: number };

function tracked(item: InboxItem): boolean {
  return (
    item.provider === "github" && (item.kind === "issue" || item.kind === "pr")
  );
}

function snapshotKey(item: InboxItem): string {
  return JSON.stringify([
    inboxNotificationProject(item).id,
    item.kind,
    item.number,
  ]);
}

function stateOf(item: InboxItem): string {
  return item.state.trim().toLowerCase();
}

function addedLabels(previous: InboxItem, current: InboxItem): string[] {
  const had = new Set(
    previous.labels.map((label) => label.name.trim().toLowerCase()),
  );
  return current.labels
    .map((label) => label.name.trim())
    .filter((name) => name && !had.has(name.toLowerCase()));
}

function transitionBetween(
  previous: InboxItem,
  current: InboxItem,
): Exclude<InboxTransitionKind, "labeled"> | null {
  const was = stateOf(previous);
  const now = stateOf(current);
  if (now === "open") {
    if (was === "closed" || was === "merged") return "reopened";
    return was === "open" && previous.draft && !current.draft
      ? "ready_for_review"
      : null;
  }
  if (was !== "open") return null;
  if (now === "merged") return "merged";
  return now === "closed" ? "closed" : null;
}

/** Remembers each GitHub item's last known state to report what changed. */
export class InboxTransitionTracker {
  private snapshots = new Map<string, Snapshot>();
  private scope: string | undefined;

  observe(
    items: readonly InboxItem[],
    scope: string,
    failedProviders: readonly InboxProvider[] = [],
  ): InboxTransitionObservation {
    const scopeChanged = this.scope !== undefined && scope !== this.scope;
    this.scope = scope;
    const transitions: InboxTransition[] = [];
    const missing: InboxItem[] = [];
    if (failedProviders.includes("github")) return { transitions, missing };

    const listed = new Set<string>();
    for (const item of items) {
      if (!tracked(item)) continue;
      const key = snapshotKey(item);
      listed.add(key);
      const previous = this.snapshots.get(key);
      const transition = previous
        ? transitionBetween(previous.item, item)
        : null;
      if (transition) transitions.push({ item, transition });
      if (previous) {
        for (const label of addedLabels(previous.item, item)) {
          transitions.push({ item, transition: "labeled", label });
        }
      }
      this.snapshots.set(key, { item, listed: true, failedLookups: 0 });
    }
    for (const [key, snapshot] of this.snapshots) {
      if (!snapshot.listed || listed.has(key)) continue;
      // A different query hides items without changing them, and a closed
      // item leaving the list has nothing further to report.
      if (scopeChanged || stateOf(snapshot.item) !== "open") {
        snapshot.listed = false;
        continue;
      }
      missing.push(snapshot.item);
    }
    return { transitions, missing };
  }

  /** Records the looked-up state of an item that left the list. */
  resolve(item: InboxItem): InboxTransition | null {
    const key = snapshotKey(item);
    const previous = this.snapshots.get(key);
    if (!previous) return null;
    const transition = transitionBetween(previous.item, item);
    this.snapshots.set(key, { item, listed: false, failedLookups: 0 });
    return transition ? { item, transition } : null;
  }

  /** Records a failed lookup so a deleted item is not retried forever. */
  unresolved(item: InboxItem): void {
    const key = snapshotKey(item);
    const snapshot = this.snapshots.get(key);
    if (!snapshot) return;
    snapshot.failedLookups += 1;
    if (snapshot.failedLookups >= MAX_FAILED_LOOKUPS) this.snapshots.delete(key);
  }
}

export async function resolveMissingTransitions(
  tracker: InboxTransitionTracker,
  missing: readonly InboxItem[],
  lookup: (item: InboxItem) => Promise<GithubWorkItem>,
  limit = MAX_LOOKUPS_PER_POLL,
): Promise<InboxTransition[]> {
  const transitions: InboxTransition[] = [];
  for (const item of missing.slice(0, limit)) {
    try {
      const fresh = await lookup(item);
      const transition = tracker.resolve({ ...item, ...fresh, kind: item.kind });
      if (transition) transitions.push(transition);
    } catch {
      tracker.unresolved(item);
    }
  }
  return transitions;
}
