import type { GithubWorkItem, InboxItem, InboxProvider } from "./githubTasks";
import { inboxNotificationProject } from "../../notifications/model/notificationProjects";

export type InboxTransitionKind =
  "reopened" | "closed" | "merged" | "ready_for_review" | "head_changed";

export type InboxTransition = {
  item: InboxItem;
  transition: InboxTransitionKind;
  previousHead?: string;
};

export type InboxTransitionObservation = {
  transitions: InboxTransition[];
  /** Open items that left the list; only a lookup can tell closed from hidden. */
  missing: InboxItem[];
};

const HEAD_STORAGE_KEY = "monocode.inbox-pr-heads.v1";
type HeadSnapshot = { headRefOid: string; state: string };

function loadHeads(): Map<string, HeadSnapshot> {
  try {
    const entries: unknown = JSON.parse(
      localStorage.getItem(HEAD_STORAGE_KEY) ?? "[]",
    );
    if (!Array.isArray(entries)) return new Map();
    return new Map(
      entries.filter(
        (entry) =>
          Array.isArray(entry) &&
          typeof entry[0] === "string" &&
          typeof entry[1]?.headRefOid === "string" &&
          typeof entry[1]?.state === "string",
      ),
    );
  } catch {
    return new Map();
  }
}

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
    item.repo.toLowerCase(),
    item.kind,
    item.number,
  ]);
}

function stateOf(item: InboxItem): string {
  return item.state.trim().toLowerCase();
}

function transitionBetween(
  previous: InboxItem,
  current: InboxItem,
): InboxTransitionKind | null {
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
  private heads = loadHeads();
  revision = 0;

  /** Call only after handing transitions to the durable automation retry queue. */
  checkpoint(): void {
    try {
      localStorage.setItem(HEAD_STORAGE_KEY, JSON.stringify([...this.heads]));
    } catch {
      // In-memory detection continues; persistent catch-up requires storage.
    }
  }

  private headChange(item: InboxItem): InboxTransition | null {
    if (item.provider !== "github" || item.kind !== "pr" || !item.headRefOid)
      return null;
    const key = snapshotKey(item);
    const previous = this.heads.get(key);
    this.heads.set(key, { headRefOid: item.headRefOid, state: stateOf(item) });
    if (
      !previous ||
      previous.state !== "open" ||
      stateOf(item) !== "open" ||
      previous.headRefOid === item.headRefOid
    )
      return null;
    return {
      item,
      transition: "head_changed",
      previousHead: previous.headRefOid,
    };
  }

  observe(
    items: readonly InboxItem[],
    scope: string,
    failedProviders: readonly InboxProvider[] = [],
  ): InboxTransitionObservation {
    this.revision += 1;
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
      const head = this.headChange(item);
      if (head) transitions.push(head);
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
  resolve(item: InboxItem, expectedRevision?: number): InboxTransition[] {
    const key = snapshotKey(item);
    const previous = this.snapshots.get(key);
    if (
      !previous ||
      (expectedRevision !== undefined &&
        (this.revision !== expectedRevision || !previous.listed))
    )
      return [];
    const transition = transitionBetween(previous.item, item);
    this.snapshots.set(key, { item, listed: false, failedLookups: 0 });
    const head = this.headChange(item);
    return [
      ...(transition ? [{ item, transition }] : []),
      ...(head ? [head] : []),
    ];
  }

  /** Records a failed lookup so a deleted item is not retried forever. */
  unresolved(item: InboxItem, expectedRevision?: number): void {
    const key = snapshotKey(item);
    const snapshot = this.snapshots.get(key);
    if (
      !snapshot ||
      (expectedRevision !== undefined &&
        (this.revision !== expectedRevision || !snapshot.listed))
    )
      return;
    snapshot.failedLookups += 1;
    if (snapshot.failedLookups >= MAX_FAILED_LOOKUPS)
      this.snapshots.delete(key);
  }
}

export async function resolveMissingTransitions(
  tracker: InboxTransitionTracker,
  missing: readonly InboxItem[],
  lookup: (item: InboxItem) => Promise<GithubWorkItem>,
  limit = MAX_LOOKUPS_PER_POLL,
  isCurrent: () => boolean = () => true,
): Promise<InboxTransition[]> {
  const transitions: InboxTransition[] = [];
  const revision = tracker.revision;
  for (const item of missing.slice(0, limit)) {
    if (tracker.revision !== revision || !isCurrent()) break;
    try {
      const fresh = await lookup(item);
      if (!isCurrent()) break;
      transitions.push(
        ...tracker.resolve({ ...item, ...fresh, kind: item.kind }, revision),
      );
    } catch {
      if (!isCurrent()) break;
      tracker.unresolved(item, revision);
    }
  }
  return transitions;
}
