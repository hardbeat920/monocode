/** Sessions that finished while unfocused, until the user looks at them. */

export function nextUnseenFinishedSessions({
  previousBusyIds,
  busyIds,
  previousUnseenIds,
  focusedSessionId,
  untrackedIds,
}: {
  previousBusyIds: ReadonlySet<string>;
  busyIds: ReadonlySet<string>;
  previousUnseenIds: ReadonlySet<string>;
  focusedSessionId?: string;
  /** Sessions the user cannot focus or see as done, such as orchestration
   * workers and inbox discussions. An unseen session stays loaded until it
   * is looked at, so marking one of these would keep it in memory for good. */
  untrackedIds?: ReadonlySet<string>;
}): Set<string> {
  const next = new Set(previousUnseenIds);
  for (const id of previousBusyIds) {
    if (!busyIds.has(id) && id !== focusedSessionId) next.add(id);
  }
  for (const id of busyIds) next.delete(id);
  if (focusedSessionId) next.delete(focusedSessionId);
  if (untrackedIds) for (const id of untrackedIds) next.delete(id);
  return next;
}

/**
 * How many replies each session finished while unfocused, until the user
 * looks at it. Unlike the done set, a count survives the session working
 * again, so replies keep adding up while the user is away.
 */
export function nextUnseenReplyCounts({
  previousBusyIds,
  busyIds,
  previousCounts,
  focusedSessionId,
  trackedIds,
}: {
  previousBusyIds: ReadonlySet<string>;
  busyIds: ReadonlySet<string>;
  previousCounts: ReadonlyMap<string, number>;
  focusedSessionId?: string;
  /** Only these sessions are counted; others are dropped. */
  trackedIds: ReadonlySet<string>;
}): Map<string, number> {
  const next = new Map(previousCounts);
  for (const id of previousBusyIds) {
    if (!busyIds.has(id) && id !== focusedSessionId && trackedIds.has(id))
      next.set(id, (next.get(id) ?? 0) + 1);
  }
  if (focusedSessionId) next.delete(focusedSessionId);
  for (const id of next.keys()) if (!trackedIds.has(id)) next.delete(id);
  return next;
}
