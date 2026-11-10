import { useEffect, useState } from "react";

import type { SessionSummary } from "@/features/workspace/types";

/** How long a new session takes to come in and push the rows below it down. */
export const ARRIVE_MS = 280;

const NONE: ReadonlySet<string> = new Set();

/**
 * The sessions to list, and those that just came in at the top of it, to
 * slide in. A row only moves smoothly if it has its transition before its
 * place changes, so a new session is held out for one render while the list
 * arms its rows; then it is let in and named in `arriving` until it settles.
 *
 * `scope` names what the list shows (the project, search, filter, and whether
 * its first page has landed): a change there swaps the list rather than adding
 * to it, so nothing slides. Nor does anything while `live` is off, as when the
 * page is out of view.
 */
export function useArrivals(visible: SessionSummary[], scope: string, live: boolean) {
  const [shown, setShown] = useState(() => ({ scope, ids: idsOf(visible) }));
  const [arriving, setArriving] = useState(NONE);

  const fresh = live && shown.scope === scope ? newAtTop(visible, shown.ids) : [];
  // Any other change to the list is taken as it is.
  const held = new Set(fresh);
  if (shown.scope !== scope || visible.some((s) => !shown.ids.has(s.id) && !held.has(s.id))) {
    setShown({ scope, ids: new Set([...idsOf(visible)].filter((id) => !held.has(id))) });
  }

  const freshKey = fresh.join("\n");
  useEffect(() => {
    if (!freshKey) return;
    const ids = freshKey.split("\n");
    // The held render has committed and armed the rows; let the new ones in.
    const frame = requestAnimationFrame(() => {
      setShown((previous) => ({ ...previous, ids: new Set([...previous.ids, ...ids]) }));
      setArriving(new Set(ids));
    });
    return () => cancelAnimationFrame(frame);
  }, [freshKey]);

  useEffect(() => {
    if (!arriving.size) return;
    const timer = setTimeout(() => setArriving(NONE), ARRIVE_MS);
    return () => clearTimeout(timer);
  }, [arriving]);

  return {
    sessions: held.size ? visible.filter((session) => !held.has(session.id)) : visible,
    arriving,
    /** Whether the rows should move to their new places rather than jump. */
    moving: held.size > 0 || arriving.size > 0,
  };
}

function idsOf(sessions: SessionSummary[]) {
  return new Set(sessions.map((session) => session.id));
}

/** The ids at the head of `visible` that weren't listed before. */
function newAtTop(visible: SessionSummary[], known: Set<string>) {
  const fresh: string[] = [];
  for (const session of visible) {
    if (known.has(session.id)) break;
    fresh.push(session.id);
  }
  // A list with nothing known yet is a new one, not one that grew.
  return fresh.length === visible.length ? [] : fresh;
}
