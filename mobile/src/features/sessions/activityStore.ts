import { useMemo, useSyncExternalStore } from "react";

import type { SessionSummary } from "@/features/workspace/types";

/** How long a session that starts working elsewhere is marked as new. */
export const ARRIVAL_MS = 12_000;
/** Finished sessions kept on the shelf, newest first. */
const FINISHED_KEPT = 8;
/** How long a finished session may be missing from the desktop's list before
 *  it's taken as deleted there. The list can drop a session for a poll or
 *  two, say one just finished and not yet saved, or a read that failed. */
const MISSING_MS = 60_000;

export type FinishedSession = { session: SessionSummary; at: number };

type Activity = {
  /** Sessions at work, in the order they started, so a list of them stays put
   *  as their times change. */
  working: SessionSummary[];
  /** Sessions that stopped working since the app saw them at it, newest
   *  first, until opened or put away. */
  finished: FinishedSession[];
  /** Sessions that started working away from this phone, by when. */
  arrivals: Record<string, number>;
};

const EMPTY: Activity = { working: [], finished: [], arrivals: {} };

let activity = EMPTY;
/** Each session as last seen; null until the first list, which is no news. */
let seen: Map<string, SessionSummary> | null = null;
/** Sessions set to work from this phone, so they aren't marked as arrivals. */
const startedHere = new Set<string>();
/** Sessions stopped from this phone, which aren't news when they finish. */
const stoppedHere = new Set<string>();
/** Sessions open on screen, which the user sees finish for themselves. */
const viewing = new Map<string, number>();
const arrivalTimers = new Map<string, ReturnType<typeof setTimeout>>();
/** Each missing finished session's grace period, independent of catalog updates. */
const missing = new Map<string, { since: number; timer: ReturnType<typeof setTimeout> }>();
const listeners = new Set<() => void>();

const isWorking = (session: SessionSummary) => !!session.busy || !!session.waiting;

function publish(next: Activity) {
  for (const id of missing.keys()) {
    if (!next.finished.some((entry) => entry.session.id === id)) clearMissing(id);
  }
  activity = next;
  listeners.forEach((notify) => notify());
}

function clearMissing(sessionId: string) {
  clearTimeout(missing.get(sessionId)?.timer);
  missing.delete(sessionId);
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** A message or a new session sent from here: its work is no arrival. */
export function markStartedHere(sessionId: string) {
  startedHere.add(sessionId);
}

/** A stop pressed here: the session won't land on the finished shelf. */
export function markStoppedHere(sessionId: string) {
  stoppedHere.add(sessionId);
}

/** A session's screen is up; until the returned function, its finish is no news. */
export function markViewing(sessionId: string) {
  viewing.set(sessionId, (viewing.get(sessionId) ?? 0) + 1);
  dismissFinished(sessionId);
  return () => {
    const left = (viewing.get(sessionId) ?? 1) - 1;
    if (left > 0) viewing.set(sessionId, left);
    else viewing.delete(sessionId);
  };
}

/** Takes a session off the finished shelf, as it's opened or put away. */
export function dismissFinished(sessionId: string) {
  if (!activity.finished.some((entry) => entry.session.id === sessionId)) return;
  publish({
    ...activity,
    finished: activity.finished.filter((entry) => entry.session.id !== sessionId),
  });
}

export function dismissAllFinished(projectId: string) {
  const finished = activity.finished.filter(
    (entry) => entry.session.projectId !== projectId,
  );
  if (finished.length !== activity.finished.length) publish({ ...activity, finished });
}

function endArrival(sessionId: string) {
  clearTimeout(arrivalTimers.get(sessionId));
  arrivalTimers.delete(sessionId);
  if (!(sessionId in activity.arrivals)) return;
  const { [sessionId]: _, ...arrivals } = activity.arrivals;
  publish({ ...activity, arrivals });
}

/**
 * Compares the desktop's latest session list with the last: a session that
 * starts working joins the end of `working` (an arrival, unless this phone
 * set it going), and one that stops goes onto the finished shelf (unless
 * stopped from here, or open on screen). SessionActivity feeds it.
 */
export function trackSessions(sessions: SessionSummary[], now = Date.now()) {
  const before = seen;
  seen = new Map(sessions.map((session) => [session.id, session]));
  const byId = seen;
  if (!before) {
    publish({ ...EMPTY, working: sessions.filter(isWorking) });
    return;
  }

  const arrivals = { ...activity.arrivals };
  let finished = activity.finished;
  const started: SessionSummary[] = [];
  for (const session of sessions) {
    const last = before.get(session.id);
    const wasWorking = !!last && isWorking(last);
    if (isWorking(session) && !wasWorking) {
      started.push(session);
      // Back at work, it's off the shelf.
      finished = finished.filter((entry) => entry.session.id !== session.id);
      if (!startedHere.has(session.id)) {
        arrivals[session.id] = now;
        clearTimeout(arrivalTimers.get(session.id));
        arrivalTimers.set(
          session.id,
          setTimeout(() => endArrival(session.id), ARRIVAL_MS),
        );
      }
    } else if (!isWorking(session) && wasWorking) {
      startedHere.delete(session.id);
      delete arrivals[session.id];
      if (stoppedHere.delete(session.id) || viewing.has(session.id)) continue;
      finished = [
        { session, at: now },
        ...finished.filter((entry) => entry.session.id !== session.id),
      ].slice(0, FINISHED_KEPT);
    }
  }

  // The ones still at work keep their places, with their latest details.
  const working = [
    ...activity.working.flatMap((session) => {
      const latest = byId.get(session.id);
      return latest && isWorking(latest) ? [latest] : [];
    }),
    ...started,
  ];
  // Gone from the desktop a while, gone from the shelf; the rest take their
  // latest titles.
  finished = finished.flatMap((entry) => {
    const id = entry.session.id;
    const latest = byId.get(id);
    if (!latest) {
      let absent = missing.get(id);
      if (!absent) {
        absent = { since: now, timer: setTimeout(() => dismissFinished(id), MISSING_MS) };
        missing.set(id, absent);
      }
      return now - absent.since >= MISSING_MS ? [] : [entry];
    }
    clearMissing(id);
    return [latest === entry.session ? entry : { ...entry, session: latest }];
  });
  for (const id of Object.keys(arrivals)) if (!byId.has(id)) delete arrivals[id];

  const changed =
    !sameList(working, activity.working) ||
    !sameList(
      finished.map((entry) => entry.session),
      activity.finished.map((entry) => entry.session),
    ) ||
    Object.keys(arrivals).join() !== Object.keys(activity.arrivals).join();
  if (changed) publish({ working, finished, arrivals });
}

function sameList(a: SessionSummary[], b: SessionSummary[]) {
  return a.length === b.length && a.every((session, i) => session === b[i]);
}

/** Forgets everything, as another desktop is paired. */
export function resetSessionActivity() {
  seen = null;
  startedHere.clear();
  stoppedHere.clear();
  viewing.clear();
  for (const id of missing.keys()) clearMissing(id);
  arrivalTimers.forEach(clearTimeout);
  arrivalTimers.clear();
  publish(EMPTY);
}

/** Every project's activity as it stands, outside React. */
export function sessionActivity() {
  return activity;
}

/** A project's sessions at work, finished, and newly arrived. */
export function useSessionActivity(projectId: string) {
  const current = useSyncExternalStore(subscribe, () => activity);
  return useMemo(
    () => ({
      working: current.working.filter((session) => session.projectId === projectId),
      finished: current.finished.filter(
        (entry) => entry.session.projectId === projectId,
      ),
      arrivals: current.arrivals,
    }),
    [current, projectId],
  );
}
