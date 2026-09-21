/** Browser-style visit stack for workspace tabs. */

const MAX_STACK = 50;

export type TabVisitHistory = {
  back: string[];
  forward: string[];
  current: string;
};

export function emptyTabVisitHistory(current: string): TabVisitHistory {
  return { back: [], forward: [], current };
}

export function canTabVisitBack(history: TabVisitHistory): boolean {
  return history.back.length > 0;
}

export function canTabVisitForward(history: TabVisitHistory): boolean {
  return history.forward.length > 0;
}

export function recordTabVisit(
  history: TabVisitHistory,
  id: string,
): TabVisitHistory {
  if (history.current === id) return history;
  return {
    back: pushVisit(history.back, history.current),
    forward: [],
    current: id,
  };
}

export function tabVisitBack(history: TabVisitHistory): TabVisitHistory | null {
  if (history.back.length === 0) return null;
  const back = history.back.slice(0, -1);
  const id = history.back[history.back.length - 1];
  return {
    back,
    forward: [history.current, ...history.forward],
    current: id,
  };
}

export function tabVisitForward(
  history: TabVisitHistory,
): TabVisitHistory | null {
  if (history.forward.length === 0) return null;
  const [id, ...forward] = history.forward;
  return {
    back: pushVisit(history.back, history.current),
    forward,
    current: id,
  };
}

/** Drop closed tabs and snap `current` onto an open id. */
export function pruneTabVisitHistory(
  history: TabVisitHistory,
  openIds: ReadonlySet<string>,
  activeId: string,
): TabVisitHistory {
  const back = dropMissing(history.back, openIds);
  const forward = dropMissing(history.forward, openIds);
  const current = openIds.has(history.current)
    ? history.current
    : openIds.has(activeId)
      ? activeId
      : (back[back.length - 1] ?? forward[0] ?? activeId);
  return collapseAdjacent({ back, forward, current });
}

function pushVisit(stack: string[], id: string): string[] {
  if (stack[stack.length - 1] === id) return stack;
  const next = [...stack, id];
  return next.length > MAX_STACK ? next.slice(-MAX_STACK) : next;
}

function dropMissing(stack: string[], openIds: ReadonlySet<string>): string[] {
  return stack.filter((id) => openIds.has(id));
}

function collapseAdjacent(history: TabVisitHistory): TabVisitHistory {
  const back = [...history.back];
  while (back[back.length - 1] === history.current) back.pop();
  const forward = [...history.forward];
  while (forward[0] === history.current) forward.shift();
  return { ...history, back, forward };
}
