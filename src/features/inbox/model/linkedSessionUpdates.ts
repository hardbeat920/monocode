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

export type LinkedSessionSeenAt = (
  sessionId: string,
  item: Pick<LinkedWorkItem, "repo" | "kind" | "number">,
  primaryItem?: Pick<LinkedWorkItem, "repo" | "kind" | "number"> | null,
) => number;

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
  seenAt: LinkedSessionSeenAt = () => 0,
): Map<string, LinkedSessionUpdate> {
  const updates = new Map<string, LinkedSessionUpdate>();
  for (const session of sessions) {
    if (session.archived) continue;
    let best: LinkedSessionUpdate | undefined;
    for (const linked of sessionLinkedWorkItems(session)) {
      const item = workItems.get(linkedWorkItemUpdateKey(linked));
      if (!item) continue;
      const since = Math.max(
        session.updatedAt,
        seenAt(session.id, linked, session.linkedWorkItem),
      );
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
  seenAt?: LinkedSessionSeenAt,
): Set<string> {
  return new Set(linkedSessionUpdates(sessions, workItems, seenAt).keys());
}

function linkedSessionUpdateSelectionKey(update: LinkedSessionUpdate): string {
  return `${linkedWorkItemUpdateKey(update.item)}:${update.updatedAt}`;
}

/** Selected linked-item updates that changed for sessions already open. */
export function linkedSessionUpdatesToReveal(
  openSessionIds: readonly string[],
  updates: ReadonlyMap<string, LinkedSessionUpdate>,
  previousSelectionKeys: ReadonlyMap<string, string>,
): {
  reveal: LinkedSessionUpdate[];
  selectionKeys: Map<string, string>;
} {
  const selectionKeys = new Map<string, string>();
  const reveal: LinkedSessionUpdate[] = [];
  for (const sessionId of new Set(openSessionIds)) {
    const update = updates.get(sessionId);
    const key = update ? linkedSessionUpdateSelectionKey(update) : "";
    selectionKeys.set(sessionId, key);
    const previous = previousSelectionKeys.get(sessionId);
    if (previous === undefined || previous === key || !update) continue;
    reveal.push(update);
  }
  return { reveal, selectionKeys };
}
