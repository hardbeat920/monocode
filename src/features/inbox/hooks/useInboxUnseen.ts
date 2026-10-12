import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  inboxListCacheKey,
  inboxItemKey,
  inboxProjectsForRail,
  githubWorkItem,
  listInboxItems,
  type GithubWorkItem,
  type InboxItem,
  type InboxQuery,
  type InboxProvider,
} from "../model/githubTasks";
import {
  applyInboxFilters,
  inboxFetchState,
  loadInboxFilters,
  pruneInboxFilters,
} from "../model/inboxFilters";
import {
  inboxHasUnseenItems,
  rememberInboxItems,
  markInboxItemsSeen,
  seedInboxSeenIfNeeded,
  subscribeInboxSeen,
  type InboxSeenEntry,
} from "../model/inboxSeen";
import {
  linkedSessionUpdates,
  linkedWorkItemUpdateKey,
  type LinkedSessionUpdate,
} from "../model/linkedSessionUpdates";
import {
  markLinkedSessionUpdateSeen,
  linkedSessionSeenAt,
  subscribeLinkedSessionSeen,
} from "../model/linkedSessionSeen";
import { loadHiddenLinearTeamIds } from "../model/linear";
import { JIRA_CHANGE_EVENT, loadHiddenJiraProjectIds } from "../model/jira";
import type { RecentProject } from "../../projects/model/recents";
import type { SessionSummary } from "../../sessions/data/sessionStore";
import { playCue } from "../../settings/model/sounds";
import {
  InboxNotificationTracker,
  inboxNotificationSubject,
} from "../model/inboxNotifications";
import {
  InboxTransitionTracker,
  resolveMissingTransitions,
  type InboxTransition,
} from "../model/inboxTransitions";
import {
  inboxNotificationProject,
  rememberNotificationProjects,
} from "../../notifications/model/notificationProjects";
import {
  allowsProjectNotificationIndicator,
  loadNotificationPreferences,
  subscribeNotificationPreferences,
  type NotificationSubject,
} from "../../notifications/model/notificationPreferences";
import {
  consumeInboxSelfActivity,
  subscribeInboxSelfActivity,
} from "../model/inboxSelfActivity";

const POLL_MS = 2 * 60_000;
const HIDDEN_POLL_MS = 5 * 60_000;
const POLL_TICK_MS = 30_000;

type ProjectSeenEntry = InboxSeenEntry & NotificationSubject;

function seenEntries(items: readonly InboxItem[]): ProjectSeenEntry[] {
  return items.map((item) => ({
    key: inboxItemKey(item),
    updatedAt: item.updatedAt,
    ...inboxNotificationSubject(item),
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
  options?: {
    /** Newly appeared items, and GitHub items that changed state. */
    onActivity?: (
      appeared: InboxItem[],
      transitions: InboxTransition[],
      observed: InboxItem[],
    ) => void;
  },
): InboxActivity {
  const [unseen, setUnseen] = useState(false);
  const [workItems, setWorkItems] = useState<
    ReadonlyMap<string, GithubWorkItem>
  >(() => new Map());
  const [linkedSeenRevision, setLinkedSeenRevision] = useState(0);
  const [notificationRevision, setNotificationRevision] = useState(0);
  const entriesRef = useRef<ProjectSeenEntry[]>([]);
  const notifications = useRef(new InboxNotificationTracker());
  const transitions = useRef(new InboxTransitionTracker());
  const sessionsRef = useRef(sessions);
  const onActivityRef = useRef(options?.onActivity);
  const lastPulledAt = useRef<number | null>(null);

  const applyUnseen = useCallback(() => {
    const preferences = loadNotificationPreferences();
    // Category choices and mute affect badges, never the item's unread state.
    setUnseen(
      inboxHasUnseenItems(
        entriesRef.current.filter((entry) =>
          allowsProjectNotificationIndicator(entry, preferences),
        ),
      ),
    );
  }, []);

  useLayoutEffect(() => {
    sessionsRef.current = sessions;
  }, [sessions]);

  useLayoutEffect(() => {
    onActivityRef.current = options?.onActivity;
  }, [options?.onActivity]);

  useEffect(() => {
    const stopSeen = subscribeInboxSeen(applyUnseen);
    const stopPreferences = subscribeNotificationPreferences(() => {
      applyUnseen();
      setNotificationRevision((revision) => revision + 1);
    });
    return () => {
      stopSeen();
      stopPreferences();
    };
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
      applyUnseen();
      return;
    }

    let cancelled = false;
    let pulling = false;
    let pullAgain = false;

    const pull = async (force: boolean) => {
      if (pulling) {
        pullAgain ||= force;
        return;
      }
      pulling = true;
      lastPulledAt.current = Date.now();
      const projectPaths = projects.map((project) => project.path);
      const filters = pruneInboxFilters(loadInboxFilters(), projectPaths);
      const query: InboxQuery = {
        assignedToMe: filters.assignedToMe,
        state: inboxFetchState(filters),
        search: "",
        linearHiddenTeamIds: loadHiddenLinearTeamIds(),
        jiraHiddenProjectIds: loadHiddenJiraProjectIds(),
      };
      try {
        const listed = await listInboxItems(projects, query, { force });
        if (cancelled) return;
        const visible = applyInboxFilters(listed.items, filters, "");
        rememberNotificationProjects(
          listed.items.map(inboxNotificationProject),
        );
        const scope = inboxListCacheKey(projects, query);
        const failedProviders = Object.keys(listed.errors) as InboxProvider[];
        const observed = notifications.current.observe(
          listed.items,
          scope,
          failedProviders,
        );
        const moved = transitions.current.observe(
          listed.items,
          scope,
          failedProviders,
        );
        const changed = observed.changed;
        // Invoke on every successful poll so retained automation claims can be
        // retried even when the item is no longer newly appeared.
        onActivityRef.current?.(
          observed.appeared,
          moved.transitions,
          failedProviders.includes("github") ? [] : listed.items,
        );
        transitions.current.checkpoint();
        const selfAuthored = changed.filter((item) =>
          consumeInboxSelfActivity(item),
        );
        const selfAuthoredKeys = new Set(selfAuthored.map(inboxItemKey));
        const visibleKeys = new Set(visible.map(inboxItemKey));
        // The whole batch is observed even when every cue is suppressed. At most
        // one eligible project chimes; muted projects cannot consume that slot.
        for (const item of changed) {
          if (selfAuthoredKeys.has(inboxItemKey(item))) continue;
          if (!visibleKeys.has(inboxItemKey(item))) continue;
          if (playCue("inboxUnseen", inboxNotificationSubject(item))) break;
        }
        const entries = seenEntries(visible);
        entriesRef.current = entries;
        rememberInboxItems(
          listed.items.map((item) => ({
            key: inboxItemKey(item),
            updatedAt: item.updatedAt,
            projectPath: item.projectPath,
          })),
        );
        seedInboxSeenIfNeeded(entries);
        const selfAuthoredEntries = entries.filter((entry) =>
          selfAuthoredKeys.has(entry.key),
        );
        if (selfAuthoredEntries.length > 0) {
          markInboxItemsSeen(selfAuthoredEntries);
        }
        for (const item of selfAuthored) {
          if (
            item.provider !== "github" ||
            (item.kind !== "issue" && item.kind !== "pr")
          )
            continue;
          const updatedAt = Date.parse(item.updatedAt);
          if (!Number.isFinite(updatedAt)) continue;
          const key = linkedWorkItemUpdateKey({
            repo: item.repo,
            kind: item.kind,
            number: item.number,
          });
          for (const session of sessionsRef.current) {
            if (
              session.linkedWorkItem &&
              linkedWorkItemUpdateKey(session.linkedWorkItem) === key
            ) {
              markLinkedSessionUpdateSeen(session.id, updatedAt);
            }
          }
        }
        applyUnseen();

        // Linked badges reuse this list. Fetching every omitted historical
        // item individually makes background traffic grow with session history.
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
          if (Number.isFinite(Date.parse(item.updatedAt))) {
            snapshots.push([key, { ...item, kind }]);
          }
        }
        if (snapshots.length > 0) {
          setWorkItems((current) => mergeSnapshots(current, snapshots));
        }
        // A closed item just leaves an open-only list, so ask GitHub what
        // happened to it. The tracker discards stale lookup completions.
        const confirmed = await resolveMissingTransitions(
          transitions.current,
          moved.missing,
          (item) =>
            githubWorkItem(
              item.projectPath,
              item.repo,
              item.kind === "pr" ? "pr" : "issue",
              item.number,
              { force: true },
            ),
          undefined,
          () => !cancelled,
        );
        // Earlier lookups may have resolved before this effect was replaced.
        // Hand those transitions off because the tracker has recorded them.
        if (confirmed.length > 0) {
          onActivityRef.current?.(
            [],
            confirmed,
            confirmed.map(({ item }) => item),
          );
        }
        transitions.current.checkpoint();
      } catch {
        // Leave the last known badges; a later poll can try again.
      } finally {
        pulling = false;
        if (!cancelled && pullAgain) {
          pullAgain = false;
          void pull(true);
        }
      }
    };

    void pull(false);
    const stopSelfActivity = subscribeInboxSelfActivity(() => void pull(true));
    // Automation triggers still run in the tray, at a slower cadence. Resume
    // events share the cadence so frequent focus changes cannot flood GitHub.
    const poll = () => {
      if (pulling) return;
      const interval = document.hidden ? HIDDEN_POLL_MS : POLL_MS;
      if (
        lastPulledAt.current != null &&
        Date.now() - lastPulledAt.current < interval
      )
        return;
      void pull(true);
    };
    const timer = window.setInterval(poll, POLL_TICK_MS);
    const onVis = () => {
      if (!document.hidden) poll();
    };
    document.addEventListener("visibilitychange", onVis);
    const onJiraChange = () => void pull(true);
    window.addEventListener(JIRA_CHANGE_EVENT, onJiraChange);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVis);
      window.removeEventListener(JIRA_CHANGE_EVENT, onJiraChange);
      stopSelfActivity();
    };
  }, [applyUnseen, cwd, recents]);

  const updates = useMemo(
    () => linkedSessionUpdates(sessions, workItems, linkedSessionSeenAt),
    [sessions, workItems, linkedSeenRevision],
  );
  const linkedIndicators = useMemo(() => {
    const preferences = loadNotificationPreferences();
    return new Set(
      [...updates]
        .filter(([, update]) =>
          allowsProjectNotificationIndicator(
            inboxNotificationSubject({ ...update.item, provider: "github" }),
            preferences,
          ),
        )
        .map(([id]) => id),
    );
  }, [updates, notificationRevision]);
  return {
    unseen,
    linkedSessionUpdates: updates,
    linkedSessionUpdateIds: linkedIndicators,
  };
}

/** Badge-only compatibility wrapper for consumers that do not render sessions. */
export function useInboxUnseen(recents: RecentProject[], cwd: string): boolean {
  return useInboxActivity(recents, cwd, []).unseen;
}
