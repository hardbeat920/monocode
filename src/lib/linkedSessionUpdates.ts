import type { LinkedWorkItem } from "./session";
import type { GithubWorkItem } from "./githubTasks";
import type { SessionSummary } from "./sessionStore";

export type LinkedWorkItemTarget = {
  key: string;
  item: LinkedWorkItem;
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

export function linkedWorkItemTargets(
  sessions: readonly SessionSummary[],
): LinkedWorkItemTarget[] {
  const targets = new Map<string, LinkedWorkItem>();
  for (const session of sessions) {
    if (!session.linkedWorkItem || session.archived) continue;
    const key = linkedWorkItemUpdateKey(session.linkedWorkItem);
    if (!targets.has(key)) targets.set(key, session.linkedWorkItem);
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
    if (!linked || session.archived) continue;
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
