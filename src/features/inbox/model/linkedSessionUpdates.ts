import type {
  GithubLinkedWorkItem,
  LinkedWorkItem,
} from "../../sessions/model/session";
import type { GithubWorkItem } from "./githubTasks";
import type { SessionSummary } from "../../sessions/data/sessionStore";

export type LinkedWorkItemTarget = {
  key: string;
  item: GithubLinkedWorkItem;
};

export type LinkedSessionUpdate = {
  sessionId: string;
  item: GithubWorkItem;
  /** The last local turn or acknowledged remote snapshot, whichever is newer. */
  since: number;
  updatedAt: number;
};

export function linkedWorkItemUpdateKey(
  item: Pick<LinkedWorkItem, "repo" | "kind" | "number">,
): string {
  return `${item.repo.trim().toLowerCase()}:${item.kind}:${item.number}`;
}

/** GitHub items to poll for updates. Linear links are opened on demand only. */
export function linkedWorkItemTargets(
  sessions: readonly SessionSummary[],
): LinkedWorkItemTarget[] {
  const targets = new Map<string, GithubLinkedWorkItem>();
  for (const session of sessions) {
    const linked = session.linkedWorkItem;
    if (!linked || linked.kind === "linear" || session.archived) continue;
    const key = linkedWorkItemUpdateKey(linked);
    if (!targets.has(key)) targets.set(key, linked);
  }
  return [...targets].map(([key, item]) => ({ key, item }));
}

/** Sessions whose GitHub item changed after the last local turn/read snapshot. */
export function linkedSessionUpdates(
  sessions: readonly SessionSummary[],
  workItems: ReadonlyMap<string, GithubWorkItem>,
  seenAt: (sessionId: string) => number = () => 0,
): Map<string, LinkedSessionUpdate> {
  const updates = new Map<string, LinkedSessionUpdate>();
  for (const session of sessions) {
    const linked = session.linkedWorkItem;
    if (!linked || linked.kind === "linear" || session.archived) continue;
    const item = workItems.get(linkedWorkItemUpdateKey(linked));
    if (!item) continue;
    const remoteUpdatedAt = Date.parse(item.updatedAt);
    const since = Math.max(session.updatedAt, seenAt(session.id));
    if (Number.isFinite(remoteUpdatedAt) && remoteUpdatedAt > since) {
      updates.set(session.id, {
        sessionId: session.id,
        item,
        since,
        updatedAt: remoteUpdatedAt,
      });
    }
  }
  return updates;
}

export function linkedSessionUpdateIds(
  sessions: readonly SessionSummary[],
  workItems: ReadonlyMap<string, GithubWorkItem>,
  seenAt?: (sessionId: string) => number,
): Set<string> {
  return new Set(linkedSessionUpdates(sessions, workItems, seenAt).keys());
}
