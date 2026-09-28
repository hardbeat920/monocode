import type { LinkedWorkItem } from "../../sessions/model/session";
import { sessionLinkedWorkItems } from "../../sessions/model/sessionWorkItem";
import type { GithubWorkItem } from "./githubTasks";
import type { SessionSummary } from "../../sessions/data/sessionStore";

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
    if (session.archived) continue;
    for (const linked of sessionLinkedWorkItems(session)) {
      const key = linkedWorkItemUpdateKey(linked);
      if (!targets.has(key)) targets.set(key, linked);
    }
  }
  return [...targets].map(([key, item]) => ({ key, item }));
}

/** Sessions whose GitHub item changed after the last local turn/read snapshot. */
export function linkedSessionUpdates(
  sessions: readonly SessionSummary[],
  workItems: ReadonlyMap<string, GithubWorkItem>,
  seenAt: (
    sessionId: string,
    item: Pick<LinkedWorkItem, "repo" | "kind" | "number">,
  ) => number = () => 0,
): Map<string, LinkedSessionUpdate> {
  const updates = new Map<string, LinkedSessionUpdate>();
  for (const session of sessions) {
    if (session.archived) continue;
    let best: LinkedSessionUpdate | undefined;
    for (const linked of sessionLinkedWorkItems(session)) {
      const item = workItems.get(linkedWorkItemUpdateKey(linked));
      if (!item) continue;
      const since = Math.max(session.updatedAt, seenAt(session.id, linked));
      const remoteUpdatedAt = Date.parse(item.updatedAt);
      if (
        Number.isFinite(remoteUpdatedAt) &&
        remoteUpdatedAt > since &&
        (!best || remoteUpdatedAt > best.updatedAt)
      ) {
        best = {
          sessionId: session.id,
          item,
          since,
          updatedAt: remoteUpdatedAt,
        };
      }
    }
    if (best) updates.set(session.id, best);
  }
  return updates;
}

export function linkedSessionUpdateIds(
  sessions: readonly SessionSummary[],
  workItems: ReadonlyMap<string, GithubWorkItem>,
  seenAt?: (
    sessionId: string,
    item: Pick<LinkedWorkItem, "repo" | "kind" | "number">,
  ) => number,
): Set<string> {
  return new Set(linkedSessionUpdates(sessions, workItems, seenAt).keys());
}
