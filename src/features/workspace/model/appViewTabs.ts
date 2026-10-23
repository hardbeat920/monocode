export const APP_VIEW_LABELS = {
  search: "Search",
  inbox: "Inbox",
  notes: "Notes",
  automations: "Automations",
} as const;

export type AppView = keyof typeof APP_VIEW_LABELS;

export function appViewTabId(view: AppView): string {
  return `app-view:${view}`;
}

export function appViewForTab(id: string): AppView | undefined {
  if (!id.startsWith("app-view:")) return;
  const view = id.slice("app-view:".length);
  return Object.prototype.hasOwnProperty.call(APP_VIEW_LABELS, view)
    ? (view as AppView)
    : undefined;
}

/** A Mono open as a title tab in the inset layout. */
export function monoTabId(monoId: string): string {
  return `mono:${monoId}`;
}

export function monoForTab(id: string): string | undefined {
  return id.startsWith("mono:") ? id.slice("mono:".length) : undefined;
}

/** Whether a title tab is a view of its own rather than a workspace tab. */
export function isViewTab(id: string): boolean {
  return !!appViewForTab(id) || !!monoForTab(id);
}

/** Keep the shared order and append newly opened tabs regardless of type. */
export function reconcileTitleTabOrder(
  order: string[],
  openIds: readonly string[],
): string[] {
  const open = new Set(openIds);
  const next = order.filter((id) => open.has(id));
  const seen = new Set(next);
  for (const id of openIds) {
    if (seen.has(id)) continue;
    next.push(id);
    seen.add(id);
  }
  return next.length === order.length &&
    next.every((id, index) => id === order[index])
    ? order
    : next;
}

/** Closing the active tab keeps focus at its slot, falling left at the end. */
export function nextDisplayedTabAfterClose(
  tabs: readonly { id: string }[],
  activeId: string,
  closingId: string,
): string | undefined {
  if (closingId !== activeId) return;
  const index = tabs.findIndex((tab) => tab.id === closingId);
  if (index < 0) return;
  return tabs[index + 1]?.id ?? tabs[index - 1]?.id;
}
