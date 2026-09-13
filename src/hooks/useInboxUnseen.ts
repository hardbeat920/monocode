import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  githubWorkItem,
  inboxItemKey,
  inboxProjectsForRail,
  listInboxItems,
  type GithubWorkItem,
  type InboxItem,
  type InboxQuery,
} from "../lib/githubTasks";
import {
  applyInboxFilters,
  inboxFetchState,
  loadInboxFilters,
  pruneInboxFilters,
} from "../lib/inboxFilters";
import {
  inboxHasUnseenItems,
  seedInboxSeenIfNeeded,
  subscribeInboxSeen,
  type InboxSeenEntry,
} from "../lib/inboxSeen";
import {
  linkedSessionUpdates,
  linkedWorkItemTargets,
  linkedWorkItemUpdateKey,
  type LinkedSessionUpdate,
  type LinkedWorkItemTarget,
} from "../lib/linkedSessionUpdates";
import {
  linkedSessionSeenAt,
  subscribeLinkedSessionSeen,
} from "../lib/linkedSessionSeen";
import { loadHiddenLinearTeamIds } from "../lib/linear";
import type { RecentProject } from "../lib/recents";
import type { SessionSummary } from "../lib/sessionStore";
import { noteInboxUnseen } from "../lib/sounds";

const POLL_MS = 30_000;
const FALLBACK_REFRESH_MS = 60_000;
const MAX_CONCURRENT_LOOKUPS = 3;

function seenEntries(items: readonly InboxItem[]): InboxSeenEntry[] {
  return items.map((item) => ({
    key: inboxItemKey(item),
    updatedAt: item.updatedAt,
  }));
}

function mergeSnapshots(
  current: ReadonlyMap<string, GithubWorkItem>,
  snapshots: readonly (readonly [string, GithubWorkItem])[],
): ReadonlyMap<string, GithubWorkItem> {
  let next: Map<string, GithubWorkItem> | undefined;
  for (const [key, item] of snapshots) {
    if (current.get(key)?.updatedAt === item.updatedAt) continue;
    next ??= new Map(current);
    next.set(key, item);
  }
  return next ?? current;
}

async function fetchFallbackUpdates(
  cwd: string,
  targets: readonly LinkedWorkItemTarget[],
): Promise<Array<readonly [string, GithubWorkItem]>> {
  const results: Array<readonly [string, GithubWorkItem]> = [];
  let cursor = 0;
  const worker = async () => {
    while (cursor < targets.length) {
      const target = targets[cursor++];
      if (!target) return;
      try {
        const item = await githubWorkItem(
          cwd,
          target.item.repo,
          target.item.kind,
          target.item.number,
          { force: true },
        );
        if (Number.isFinite(Date.parse(item.updatedAt))) {
          results.push([target.key, item]);
        }
      } catch {
        // The next shared Inbox poll retries missing items.
      }
    }
  };
  await Promise.all(
    Array.from(
      { length: Math.min(MAX_CONCURRENT_LOOKUPS, targets.length) },
      worker,
    ),
  );
  return results;
}

export type InboxActivity = {
  unseen: boolean;
  linkedSessionUpdateIds: ReadonlySet<string>;
  linkedSessionUpdates: ReadonlyMap<string, LinkedSessionUpdate>;
};

/** One background refresh supplies both the Inbox badge and linked sessions. */
export function useInboxActivity(
  recents: RecentProject[],
  cwd: string,
  sessions: readonly SessionSummary[],
): InboxActivity {
  const [unseen, setUnseen] = useState(false);
  const [workItems, setWorkItems] = useState<
    ReadonlyMap<string, GithubWorkItem>
  >(() => new Map());
  const [linkedSeenRevision, setLinkedSeenRevision] = useState(0);
  const entriesRef = useRef<InboxSeenEntry[]>([]);
  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  const fallbackFetchedAt = useRef(new Map<string, number>());
  const targetKey = linkedWorkItemTargets(sessions)
    .map((target) => target.key)
    .join("\0");

  const applyUnseen = useCallback((next: boolean) => {
    noteInboxUnseen(next);
    setUnseen(next);
  }, []);

  useEffect(() => {
    return subscribeInboxSeen(() => {
      applyUnseen(inboxHasUnseenItems(entriesRef.current));
    });
  }, [applyUnseen]);

  useEffect(
    () =>
      subscribeLinkedSessionSeen(() =>
        setLinkedSeenRevision((revision) => revision + 1),
      ),
    [],
  );

  useEffect(() => {
    const projects = inboxProjectsForRail(recents, cwd);
    if (projects.length === 0) {
      entriesRef.current = [];
      applyUnseen(false);
      return;
    }

    let cancelled = false;
    let pulling = false;

    const pull = async (force: boolean) => {
      if (pulling) return;
      pulling = true;
      const projectPaths = projects.map((project) => project.path);
      const filters = pruneInboxFilters(loadInboxFilters(), projectPaths);
      const query: InboxQuery = {
        assignedToMe: filters.assignedToMe,
        state: inboxFetchState(filters),
        search: "",
        linearHiddenTeamIds: loadHiddenLinearTeamIds(),
      };
      try {
        const listed = await listInboxItems(projects, query, { force });
        if (cancelled) return;
        const visible = applyInboxFilters(listed.items, filters, "");
        const entries = seenEntries(visible);
        entriesRef.current = entries;
        seedInboxSeenIfNeeded(entries);
        applyUnseen(inboxHasUnseenItems(entries));

        const targets = linkedWorkItemTargets(sessionsRef.current);
        const targetKeys = new Set(targets.map((target) => target.key));
        const listedKeys = new Set<string>();
        const snapshots: Array<readonly [string, GithubWorkItem]> = [];
        for (const item of listed.items) {
          const kind = item.kind;
          if (
            item.provider !== "github" ||
            (kind !== "issue" && kind !== "pr")
          ) {
            continue;
          }
          const key = linkedWorkItemUpdateKey({
            repo: item.repo,
            kind,
            number: item.number,
          });
          if (!targetKeys.has(key)) continue;
          listedKeys.add(key);
          if (Number.isFinite(Date.parse(item.updatedAt))) {
            snapshots.push([key, { ...item, kind }]);
          }
        }
        if (snapshots.length > 0) {
          setWorkItems((current) => mergeSnapshots(current, snapshots));
        }

        const now = Date.now();
        const missing = targets.filter((target) => {
          if (listedKeys.has(target.key)) return false;
          const last = fallbackFetchedAt.current.get(target.key) ?? 0;
          if (now - last < FALLBACK_REFRESH_MS) return false;
          fallbackFetchedAt.current.set(target.key, now);
          return true;
        });
        if (missing.length > 0) {
          const fallback = await fetchFallbackUpdates(cwd, missing);
          if (!cancelled && fallback.length > 0) {
            setWorkItems((current) => mergeSnapshots(current, fallback));
          }
        }
      } catch {
        // Leave the last known badges; a later poll can try again.
      } finally {
        pulling = false;
      }
    };

    void pull(false);
    const timer = window.setInterval(() => {
      if (document.hidden) return;
      void pull(true);
    }, POLL_MS);
    const onVis = () => {
      if (!document.hidden) void pull(true);
    };
    document.addEventListener("visibilitychange", onVis);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVis);
    };
  }, [applyUnseen, cwd, recents, targetKey]);

  const updates = useMemo(
    () => linkedSessionUpdates(sessions, workItems, linkedSessionSeenAt),
    [sessions, workItems, linkedSeenRevision],
  );
  return {
    unseen,
    linkedSessionUpdates: updates,
    linkedSessionUpdateIds: new Set(updates.keys()),
  };
}

/** Badge-only compatibility wrapper for consumers that do not render sessions. */
export function useInboxUnseen(recents: RecentProject[], cwd: string): boolean {
  return useInboxActivity(recents, cwd, []).unseen;
}
