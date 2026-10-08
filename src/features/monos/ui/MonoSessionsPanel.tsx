import { useEffect, useState, type ReactNode } from "react";
import { projectName } from "../../../shared/lib/paths";
import { ChevronRight } from "../../../shared/ui/icons";
import {
  getSession,
  listSessionsByProject,
  type SessionSummary,
} from "../../sessions/data/sessionStore";
import { summaryFromSession } from "../../sessions/data/sessionHistory";
import { resolveModel } from "../../sessions/model/models";
import {
  hasPendingApproval,
  sessionDisplayTitle,
  sessionDraftBlock,
  type MonoSpawnedSession,
  type Session,
} from "../../sessions/model/session";
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";
import type { MonoLook } from "../model/mono";
import { MonoSidebar, MonoSidebarHeader } from "./MonoSidebar";

/** Sessions launched by the selected turn, including those no longer open. */
export function MonoSessionsPanel({
  agent,
  launches,
  sessions,
  history = [],
  onOpenSession,
  onClose,
  windowControls,
}: {
  agent: MonoLook;
  launches: readonly MonoSpawnedSession[];
  sessions: readonly Session[];
  history?: readonly SessionSummary[];
  onOpenSession: (sessionId: string) => void | Promise<void>;
  onClose: () => void;
  windowControls?: ReactNode;
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
  }>({ byId: new Map(), loaded: new Set(), failed: new Set() });
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

  return (
    <MonoSidebar
      open
      kind="sessions"
      label={`${agent.name} sessions`}
      color={agent.color}
      windowControls={windowControls}
    >
      <MonoSidebarHeader title="Sessions" onClose={onClose} />
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none px-3 py-3">
        <p className="px-1 pb-3 text-[11px] text-content/45">
          {launches.length} {launches.length === 1 ? "session" : "sessions"}{" "}
          launched in this turn
        </p>
        <div className="flex flex-col gap-2">
          {launches.map((launch) => {
            const live = sessions.find(
              (entry) => entry.id === launch.sessionId,
            );
            const saved =
              history.find((entry) => entry.id === launch.sessionId) ??
              stored.byId.get(launch.sessionId);
            const unavailable =
              !live && !saved && stored.loaded.has(launch.cwd);
            const session = live ?? saved;
            const harness = session?.harness ?? launch.harness;
            const model = resolveModel(
              harness,
              session?.model ?? launch.model,
            ).name;
            const title = sessionDisplayTitle(
              session?.title ?? launch.title,
              harness,
            );
            const status = unavailable
              ? "No longer available"
              : live &&
                  (hasPendingApproval(live.blocks) || live.pendingQuestion)
                ? "Needs input"
                : live?.busy
                  ? "Working"
                  : saved?.archived
                    ? "Archived"
                    : (live ? !!sessionDraftBlock(live) : saved?.draft)
                      ? "Draft"
                      : session
                        ? "Ready"
                        : stored.failed.has(launch.cwd)
                          ? "Status unavailable"
                          : "Loading…";
            return (
              <button
                key={launch.sessionId}
                type="button"
                data-mono-session={launch.sessionId}
                disabled={unavailable || !!opening}
                onClick={() => void open(launch.sessionId)}
                className="group flex w-full items-center gap-2 rounded-lg border border-stroke bg-content/3 px-3 py-2.5 text-left outline-none hover:border-content/20 hover:bg-content/6 focus-visible:ring-1 focus-visible:ring-accent disabled:cursor-default disabled:opacity-50"
              >
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[13px] font-medium text-content">
                    {title}
                  </p>
                  <div className="mt-1 flex min-w-0 items-center gap-1.5 text-[11px] text-content/45">
                    <HarnessIcon
                      harness={harness}
                      className="size-3 shrink-0"
                    />
                    <span className="truncate">{model}</span>
                    <span aria-hidden>·</span>
                    <span
                      className="truncate"
                      title={session?.cwd ?? launch.cwd}
                    >
                      {agent.projects.find(
                        (project) =>
                          project.path === (session?.cwd ?? launch.cwd),
                      )?.name ?? projectName(session?.cwd ?? launch.cwd)}
                    </span>
                  </div>
                  <p
                    className="mt-2 flex items-center gap-1.5 text-[11px] text-content/55"
                    role="status"
                  >
                    <span
                      aria-hidden
                      className={`size-1.5 rounded-full ${status === "Working" ? "animate-pulse bg-[var(--mono-color)]" : status === "Needs input" ? "bg-amber-500/70" : "bg-content/30"}`}
                    />
                    {opening === launch.sessionId ? "Opening…" : status}
                  </p>
                </div>
                <ChevronRight
                  aria-hidden
                  className="size-3.5 shrink-0 text-content/30 group-hover:text-content/60"
                />
              </button>
            );
          })}
        </div>
        {error ? (
          <p role="alert" className="px-1 pt-3 text-xs text-red-400">
            {error}
          </p>
        ) : null}
      </div>
    </MonoSidebar>
  );
}
