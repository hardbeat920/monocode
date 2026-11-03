import { useEffect, useRef, useState, type ReactNode } from "react";
import { LIST_PAGE_SIZE, listWindowSize } from "../../../shared/lib/listWindow";
import { projectName } from "../../../shared/lib/paths";
import { IconButton } from "../../../app/shell/TitleBar";
import {
  ExternalLink,
  PanelRightToggle,
  Search,
} from "../../../shared/ui/icons";
import {
  getSession,
  listSessionsByProject,
  type SessionSummary,
} from "../../sessions/data/sessionStore";
import {
  filterSessionsByArchive,
  filterSessionsByQuery,
  summaryFromSession,
} from "../../sessions/data/sessionHistory";
import {
  DEFAULT_SESSION_SIDEBAR_FILTERS,
  filterSessionsByHarness,
  hasActiveSessionFilters,
  filterSessionsByStatus,
  filterSessionsByTime,
  type SessionSidebarFilters,
} from "../../sessions/model/sessionFilters";
import { fuzzyMatch } from "../../../shared/lib/fuzzy";
import {
  hasPendingApproval,
  type MonoSpawnedSession,
  type Session,
} from "../../sessions/model/session";
import { SessionCard } from "../../../app/shell/Sidebar";
import type { MonoLook } from "../model/mono";
import { EmptyState, PageHeader } from "./monoPanelParts";

/** A launched session open over the overview, live and fully usable. */
export type ShownMonoSession = {
  sessionId: string;
  title: string;
  /** The session's pane. */
  pane: ReactNode;
};

/**
 * Saved sessions already looked up, kept across lists. Switching Monos
 * remounts the overview, and its cards should come back as they were rather
 * than reload from blank.
 */
const storedSessions = new Map<string, SessionSummary>();

/** Cards for launched sessions, with where each stands, open or saved. */
export function MonoSessionList({
  agent,
  launches,
  sessions,
  history = [],
  query = "",
  filters = DEFAULT_SESSION_SIDEBAR_FILTERS,
  onOpenSession,
}: {
  agent: MonoLook;
  launches: readonly MonoSpawnedSession[];
  sessions: readonly Session[];
  history?: readonly SessionSummary[];
  /** Narrows the cards as the sidebar's session search does. */
  query?: string;
  filters?: SessionSidebarFilters;
  onOpenSession: (sessionId: string) => void | Promise<void>;
}) {
  const launchesKey = JSON.stringify(
    launches.map((entry) => [entry.sessionId, entry.cwd]),
  );
  // Refresh saved status when a launched session closes, archives or moves.
  const liveSessionsKey = JSON.stringify(
    launches.map((launch) => {
      const session = sessions.find((entry) => entry.id === launch.sessionId);
      return session ? [session.id, session.cwd] : null;
    }),
  );
  const [stored, setStored] = useState<{
    byId: Map<string, SessionSummary>;
    loaded: Set<string>;
    failed: Set<string>;
  }>(() => ({
    byId: new Map(
      launches.flatMap((launch) => {
        const known = storedSessions.get(launch.sessionId);
        return known ? [[launch.sessionId, known] as const] : [];
      }),
    ),
    loaded: new Set(),
    failed: new Set(),
  }));
  const [opening, setOpening] = useState<string>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    let cancelled = false;
    const refs: [string, string][] = JSON.parse(launchesKey);
    const projects = [...new Set(refs.map(([, cwd]) => cwd))];
    void Promise.allSettled(
      projects.map((cwd) => listSessionsByProject(cwd)),
    ).then(async (results) => {
      if (cancelled) return;
      const byId = new Map<string, SessionSummary>();
      const loaded = new Set<string>();
      const failed = new Set<string>();
      results.forEach((result, index) => {
        if (result.status !== "fulfilled") {
          failed.add(projects[index]);
          return;
        }
        loaded.add(projects[index]);
        result.value.forEach((session) => byId.set(session.id, session));
      });
      // Project membership can change after launch; the session id stays stable.
      const missing = refs.filter(
        ([id, cwd]) => !byId.has(id) && loaded.has(cwd),
      );
      const restored = await Promise.allSettled(
        missing.map(([id]) => getSession(id)),
      );
      if (cancelled) return;
      restored.forEach((result, index) => {
        if (result.status === "fulfilled" && result.value) {
          byId.set(result.value.id, summaryFromSession(result.value));
        } else if (result.status === "rejected") {
          loaded.delete(missing[index][1]);
          failed.add(missing[index][1]);
        }
      });
      for (const [id, summary] of byId) storedSessions.set(id, summary);
      setStored({ byId, loaded, failed });
    });
    return () => {
      cancelled = true;
    };
  }, [launchesKey, liveSessionsKey]);

  const open = async (id: string) => {
    setOpening(id);
    setError(undefined);
    try {
      await onOpenSession(id);
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : "Could not open this session.",
      );
    } finally {
      setOpening(undefined);
    }
  };

  const projectLabel = (cwd: string) =>
    agent.projects.find((project) => project.path === cwd)?.name ??
    projectName(cwd);
  // Each launch as the sidebar would list it, so its search and filters
  // apply as they do there. One not yet looked up stands in from its launch.
  const rows = launches.map((launch): SessionSummary => {
    const live = sessions.find((entry) => entry.id === launch.sessionId);
    const saved =
      history.find((entry) => entry.id === launch.sessionId) ??
      stored.byId.get(launch.sessionId);
    if (live)
      return {
        ...saved,
        ...summaryFromSession(live, saved),
        updatedAt: saved?.updatedAt ?? Date.now(),
      };
    return (
      saved ?? {
        id: launch.sessionId,
        cwd: launch.cwd,
        harness: launch.harness,
        model: launch.model,
        runtimeMode: "auto",
        title: launch.title,
        createdAt: 0,
        updatedAt: 0,
      }
    );
  });
  // Launched, then deleted: its project's sessions are known, and it is not
  // among them.
  const unavailableIds = new Set(
    launches
      .filter(
        (launch) =>
          stored.loaded.has(launch.cwd) &&
          !sessions.some((entry) => entry.id === launch.sessionId) &&
          !history.some((entry) => entry.id === launch.sessionId) &&
          !stored.byId.has(launch.sessionId),
      )
      .map((launch) => launch.sessionId),
  );
  const busyIds = new Set<string>();
  const approvalIds = new Set<string>();
  const doneIds = new Set<string>();
  for (const row of rows) {
    const live = sessions.find((entry) => entry.id === row.id);
    if (live?.busy) busyIds.add(row.id);
    else if (live && (hasPendingApproval(live.blocks) || live.pendingQuestion))
      approvalIds.add(row.id);
    else if (live || row.createdAt) doneIds.add(row.id);
  }
  const filtered = filterSessionsByStatus(
    filterSessionsByTime(
      filterSessionsByHarness(
        // Archived launches stay with the rest; the filter narrows to them.
        // Deleted ones are hidden unless the filter asks for just them.
        (filters.showArchived
          ? filterSessionsByArchive(rows, true)
          : rows
        ).filter(
          (row) => unavailableIds.has(row.id) === !!filters.showUnavailable,
        ),
        filters.hiddenHarnesses,
      ),
      filters.time,
      Date.now(),
    ),
    filters.status,
    busyIds,
    approvalIds,
    doneIds,
  );
  // The project a session runs in is on its card, so it is searched too.
  const needle = query.trim();
  const matched = new Set(
    needle
      ? [
          ...filterSessionsByQuery(filtered, needle),
          ...filtered.filter(
            (row) => fuzzyMatch(needle, projectLabel(row.cwd)) != null,
          ),
        ].map((row) => row.id)
      : filtered.map((row) => row.id),
  );
  const matching = launches.filter((launch) => matched.has(launch.sessionId));
  // A page of cards mounts at a time, as in the sidebar; the sentinel below
  // asks for the next one as it nears the panel's bottom. Searching or
  // filtering starts over from the first page.
  const [limit, setLimit] = useState(LIST_PAGE_SIZE);
  const list = useRef<HTMLDivElement>(null);
  const loadMore = useRef<HTMLDivElement>(null);
  const listKey = `${needle}\0${JSON.stringify(filters)}`;
  useEffect(() => {
    setLimit(LIST_PAGE_SIZE);
    const scroller = scrollParent(list.current);
    if (scroller) scroller.scrollTop = 0;
  }, [listKey]);
  const shown = matching.slice(0, listWindowSize(matching.length, limit));
  const hasMore = shown.length < matching.length;
  useEffect(() => {
    const sentinel = loadMore.current;
    if (!hasMore || !sentinel) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting)
          setLimit((current) => current + LIST_PAGE_SIZE);
      },
      { root: scrollParent(sentinel), rootMargin: "240px" },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [hasMore, shown.length]);

  if (launches.length > 0 && matching.length === 0)
    return (
      <EmptyState icon={Search}>
        {needle
          ? "No sessions match this search."
          : hasActiveSessionFilters(filters)
            ? "No sessions match these filters."
            : // Every one it started is gone; the filter still lists them.
              `The sessions ${agent.name} started have since been deleted.`}
      </EmptyState>
    );

  return (
    <div ref={list}>
      <div className="flex flex-col gap-1">
        {shown.map((launch) => {
          const live = sessions.find((entry) => entry.id === launch.sessionId);
          const saved =
            history.find((entry) => entry.id === launch.sessionId) ??
            stored.byId.get(launch.sessionId);
          // Once it is known, it gets the sidebar's own card: model, status,
          // title, branch or worktree, and any linked PR or issue.
          const summary: SessionSummary | undefined = live
            ? {
                ...saved,
                ...summaryFromSession(live, saved),
                updatedAt: saved?.updatedAt ?? Date.now(),
              }
            : saved;
          if (summary)
            return (
              <div key={launch.sessionId} data-mono-session={launch.sessionId}>
                <SessionCard
                  session={summary}
                  modelInFooter
                  isActive={false}
                  isSelected={false}
                  busy={!!live?.busy}
                  done={false}
                  linkedUpdate={false}
                  needsApproval={
                    !!live &&
                    (hasPendingApproval(live.blocks) || !!live.pendingQuestion)
                  }
                  project={
                    agent.projects.find(
                      (project) => project.path === summary.cwd,
                    )?.name ?? projectName(summary.cwd)
                  }
                  now={Date.now()}
                  onSelect={(id) => {
                    if (!opening) void open(id);
                  }}
                />
              </div>
            );
          // Not known, either deleted or not looked up: the sidebar's card
          // still, from what the launch recorded.
          const unavailable = unavailableIds.has(launch.sessionId);
          const failed = !unavailable && stored.failed.has(launch.cwd);
          const loading = !unavailable && !failed;
          return (
            <div
              key={launch.sessionId}
              data-mono-session={loading ? undefined : launch.sessionId}
              data-mono-session-loading={loading ? launch.sessionId : undefined}
              data-mono-session-unavailable={unavailable ? "" : undefined}
              // Until it is known, hold its place without a card of another
              // look that the real one would replace a moment later.
              aria-hidden={loading || undefined}
              aria-disabled={unavailable || undefined}
              inert={loading || unavailable}
              className={
                loading ? "invisible" : unavailable ? "opacity-50" : ""
              }
            >
              <SessionCard
                session={{
                  id: launch.sessionId,
                  cwd: launch.cwd,
                  harness: launch.harness,
                  model: launch.model,
                  runtimeMode: "auto",
                  title: launch.title,
                  createdAt: 0,
                  updatedAt: 0,
                }}
                modelInFooter
                statusLabel={
                  unavailable
                    ? "Unavailable"
                    : opening === launch.sessionId
                      ? "Opening…"
                      : failed
                        ? "Status unavailable"
                        : ""
                }
                isActive={false}
                isSelected={false}
                busy={false}
                done={false}
                linkedUpdate={false}
                needsApproval={false}
                project={projectLabel(launch.cwd)}
                now={0}
                onSelect={(id) => {
                  if (failed && !opening) void open(id);
                }}
              />
            </div>
          );
        })}
      </div>
      {hasMore ? <div ref={loadMore} aria-hidden className="h-px" /> : null}
      {error ? (
        <p role="alert" className="px-1 pt-3 text-xs text-red-400">
          {error}
        </p>
      ) : null}
    </div>
  );
}

/** The nearest ancestor that scrolls, which the list pages against. */
function scrollParent(node: HTMLElement | null): HTMLElement | null {
  for (let el = node?.parentElement; el; el = el.parentElement) {
    const { overflowY } = getComputedStyle(el);
    if (overflowY === "auto" || overflowY === "scroll") return el;
  }
  return null;
}

/** A launched session open over the overview, live and fully usable. */
export function LaunchedSessionPage({
  title,
  onBack,
  onOpenInWorkspace,
  onClose,
  children,
}: {
  title: string;
  onBack: () => void;
  onOpenInWorkspace: () => void | Promise<void>;
  onClose: () => void;
  children: ReactNode;
}) {
  const [error, setError] = useState<string>();
  return (
    <>
      <PageHeader title={title} onBack={onBack}>
        <IconButton
          label="Open in workspace"
          onClick={() => {
            setError(undefined);
            void Promise.resolve(onOpenInWorkspace()).catch((cause) =>
              setError(
                cause instanceof Error
                  ? cause.message
                  : "Could not open this session.",
              ),
            );
          }}
        >
          <ExternalLink className="size-3.5" strokeWidth={1.75} />
        </IconButton>
        <IconButton label="Hide overview" active onClick={onClose}>
          <PanelRightToggle className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      </PageHeader>
      {error ? (
        <p role="alert" className="px-4 pt-2 text-xs text-red-400">
          {error}
        </p>
      ) : null}
      <div
        data-mono-launched-session=""
        className="flex min-h-0 min-w-0 flex-1 flex-col"
      >
        {children}
      </div>
    </>
  );
}
