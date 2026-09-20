import {
  closeLeaf,
  focusedFileTab,
  firstLeafId,
  leaf,
  leafIds,
  placeLayout,
  placePane,
  replacePaneWithLayout,
  replaceLeafId,
  splitPane,
  type PaneEdge,
  type WorkspaceTab,
} from "./layout";
import { projectName } from "./paths";
import { sameProjectPath } from "./recents";
import type { Session } from "./session";

export function workspaceTabCwd(
  tab: WorkspaceTab,
  sessions: readonly Pick<Session, "id" | "cwd">[],
): string | null {
  for (const id of leafIds(tab.layout)) {
    const session = sessions.find((entry) => entry.id === id);
    if (session?.cwd && session.cwd !== "~") return session.cwd;
  }

  const file = focusedFileTab(tab);
  const cwd = file?.projectCwd ?? file?.cwd;
  if (cwd && cwd !== "~") return cwd;

  return null;
}

export function focusedWorkspaceTabCwd(
  tab: WorkspaceTab,
  sessions: readonly Pick<Session, "id" | "cwd">[],
): string | null {
  const session = sessions.find((entry) => entry.id === tab.focusedId);
  const file = focusedFileTab(tab);
  return (
    session?.cwd ?? file?.projectCwd ?? file?.cwd ?? workspaceTabCwd(tab, sessions)
  );
}

export function workspaceTabProject(
  tab: WorkspaceTab,
  sessions: Session[],
): string | null {
  const cwd = workspaceTabCwd(tab, sessions);
  if (!cwd) return null;
  const name = projectName(cwd);
  return name === "~" ? null : name;
}

export function findTabForProject(
  tabs: WorkspaceTab[],
  sessions: Session[],
  path: string,
): WorkspaceTab | undefined {
  return tabs.find((tab) => {
    const cwd = workspaceTabCwd(tab, sessions);
    return cwd ? sameProjectPath(cwd, path) : false;
  });
}

/** A tab is only openable as a chat when its session object is also mounted. */
export function findOpenSessionTab(
  tabs: readonly WorkspaceTab[],
  sessions: readonly Pick<Session, "id">[],
  sessionId: string,
): WorkspaceTab | undefined {
  if (!sessions.some((session) => session.id === sessionId)) return undefined;
  return tabs.find((tab) => leafIds(tab.layout).includes(sessionId));
}

/** Add a chat beside a file-only tab so an add-to-chat request has a target. */
export function openAddToChatSessionPane({
  tab,
  sessions,
  sessionId,
}: {
  tab: WorkspaceTab;
  sessions: readonly Pick<Session, "id">[];
  sessionId: string;
}): WorkspaceTab | null {
  const paneIds = leafIds(tab.layout);
  const sessionIds = new Set(sessions.map((session) => session.id));
  if (paneIds.some((id) => sessionIds.has(id))) return null;

  const sourceId = paneIds.includes(tab.focusedId)
    ? tab.focusedId
    : firstLeafId(tab.layout);
  return {
    ...tab,
    layout: splitPane(tab.layout, sourceId, "right", sessionId),
    focusedId: sessionId,
    diffFocused: false,
  };
}

export function filterTabsForProject(
  tabs: WorkspaceTab[],
  sessions: Session[],
  path: string,
): WorkspaceTab[] {
  return tabs.filter((tab) => {
    const cwd = workspaceTabCwd(tab, sessions);
    return cwd ? sameProjectPath(cwd, path) : false;
  });
}

export type WorkspaceTabCloseScope = "project" | "workspace";

export type WorkspaceTabClosePlan =
  | { action: "keep" }
  | { action: "close"; nextActiveTabId?: string };

export function planWorkspaceTabClose({
  tabs,
  sessions,
  closingTabId,
  scope,
}: {
  tabs: WorkspaceTab[];
  sessions: Session[];
  closingTabId: string;
  scope: WorkspaceTabCloseScope;
}): WorkspaceTabClosePlan {
  const closingIndex = tabs.findIndex((tab) => tab.id === closingTabId);
  if (closingIndex < 0) return { action: "keep" };

  const remaining = tabs.filter((tab) => tab.id !== closingTabId);
  if (remaining.length === 0) return { action: "keep" };

  const globalTarget = remaining[Math.max(0, closingIndex - 1)] ?? remaining[0];
  if (scope === "workspace") {
    return { action: "close", nextActiveTabId: globalTarget?.id };
  }

  const closingCwd = workspaceTabCwd(tabs[closingIndex], sessions);
  if (!closingCwd) {
    return { action: "close", nextActiveTabId: globalTarget?.id };
  }

  for (let index = closingIndex - 1; index >= 0; index -= 1) {
    const cwd = workspaceTabCwd(tabs[index], sessions);
    if (cwd && sameProjectPath(cwd, closingCwd)) {
      return { action: "close", nextActiveTabId: tabs[index].id };
    }
  }

  for (let index = closingIndex + 1; index < tabs.length; index += 1) {
    const cwd = workspaceTabCwd(tabs[index], sessions);
    if (cwd && sameProjectPath(cwd, closingCwd)) {
      return { action: "close", nextActiveTabId: tabs[index].id };
    }
  }

  // Deck mode is one project at a time. Closing the last tab there must not
  // jump to another project's tab; the caller keeps this one instead.
  return { action: "keep" };
}

/**
 * Drop a session onto a pane edge in the tab that owns `targetId`.
 * Already-open chats move; a blank target is replaced; a chat open
 * in another tab is relocated (that tab closes when it was the last leaf).
 */
export function applyPlaceSessionOnPane({
  tabs,
  sessions,
  sessionId,
  targetId,
  edge,
  replaceTarget,
  scope,
  createReplacement,
}: {
  tabs: WorkspaceTab[];
  sessions: Session[];
  sessionId: string;
  targetId: string;
  edge: PaneEdge;
  replaceTarget: boolean;
  scope: WorkspaceTabCloseScope;
  createReplacement: (seed: Session | undefined) => Session;
}): {
  tabs: WorkspaceTab[];
  sessions: Session[];
  activeTabId: string;
} | null {
  if (sessionId === targetId) return null;
  const targetIndex = tabs.findIndex((tab) =>
    leafIds(tab.layout).includes(targetId),
  );
  if (targetIndex < 0) return null;

  let nextSessions = replaceTarget
    ? sessions.filter((session) => session.id !== targetId)
    : sessions;
  const targetTabId = tabs[targetIndex]!.id;

  let nextTabs = tabs.map((tab, index) => {
    if (index !== targetIndex) return tab;
    const layout = replaceTarget
      ? replaceLeafId(tab.layout, targetId, sessionId)
      : placePane(tab.layout, sessionId, targetId, edge);
    return { ...tab, layout, focusedId: sessionId, diffFocused: false };
  });

  for (const tab of [...nextTabs]) {
    if (tab.id === targetTabId) continue;
    if (!leafIds(tab.layout).includes(sessionId)) continue;
    const tabIndex = nextTabs.findIndex((entry) => entry.id === tab.id);
    if (tabIndex < 0) continue;

    const closed = closeLeaf(tab, sessionId);
    if (closed) {
      nextTabs[tabIndex] = closed;
      continue;
    }

    const closePlan = planWorkspaceTabClose({
      tabs: nextTabs,
      sessions: nextSessions,
      closingTabId: tab.id,
      scope,
    });
    if (closePlan.action === "close") {
      nextTabs = nextTabs.filter((entry) => entry.id !== tab.id);
      continue;
    }

    const replacement = createReplacement(
      nextSessions.find((session) => session.id === sessionId),
    );
    nextSessions = [...nextSessions, replacement];
    nextTabs[tabIndex] = {
      ...tab,
      layout: leaf(replacement.id),
      focusedId: replacement.id,
      editorPanes: [],
      terminalPanes: [],
      diffOpen: false,
      diffFocused: false,
    };
  }

  return { tabs: nextTabs, sessions: nextSessions, activeTabId: targetTabId };
}

/**
 * Drop a complete workspace tab onto a pane edge. Its split tree and surface
 * panes move together, and the source title tab is removed.
 */
export function applyPlaceTabOnPane({
  tabs,
  sessions,
  sourceTabId,
  targetId,
  edge,
  replaceTarget,
}: {
  tabs: WorkspaceTab[];
  sessions: Session[];
  sourceTabId: string;
  targetId: string;
  edge: PaneEdge;
  replaceTarget: boolean;
}): {
  tabs: WorkspaceTab[];
  sessions: Session[];
  activeTabId: string;
  focusedId: string;
} | null {
  const source = tabs.find((tab) => tab.id === sourceTabId);
  const target = tabs.find((tab) => leafIds(tab.layout).includes(targetId));
  if (!source || !target || source.id === target.id) return null;

  const targetIds = new Set(leafIds(target.layout));
  if (leafIds(source.layout).some((id) => targetIds.has(id))) return null;

  const layout = replaceTarget
    ? replacePaneWithLayout(target.layout, targetId, source.layout)
    : placeLayout(target.layout, source.layout, targetId, edge);
  const merged: WorkspaceTab = {
    ...target,
    layout,
    focusedId: source.focusedId,
    editorPanes: [...target.editorPanes, ...source.editorPanes],
    terminalPanes: [
      ...(target.terminalPanes ?? []),
      ...(source.terminalPanes ?? []),
    ],
    diffFocused: false,
  };
  const nextTabs = tabs
    .filter((tab) => tab.id !== source.id)
    .map((tab) => (tab.id === target.id ? merged : tab));
  const nextSessions = replaceTarget
    ? sessions.filter((session) => session.id !== targetId)
    : sessions;

  return {
    tabs: nextTabs,
    sessions: nextSessions,
    activeTabId: target.id,
    focusedId: source.focusedId,
  };
}

/**
 * Extract one leaf from a split workspace and turn it into a title tab.
 * Surface pane metadata moves with editor and terminal leaves.
 */
export function applyDetachPaneToTab({
  tabs,
  paneId,
  targetTabId,
  position,
  createTabId = () => crypto.randomUUID(),
}: {
  tabs: WorkspaceTab[];
  paneId: string;
  targetTabId: string;
  position: "before" | "after";
  createTabId?: () => string;
}): {
  tabs: WorkspaceTab[];
  activeTabId: string;
  focusedId: string;
} | null {
  const sourceIndex = tabs.findIndex((tab) =>
    leafIds(tab.layout).includes(paneId),
  );
  if (sourceIndex < 0 || leafIds(tabs[sourceIndex]!.layout).length < 2) {
    return null;
  }

  const source = tabs[sourceIndex]!;
  const remaining = closeLeaf(source, paneId);
  if (!remaining) return null;

  const editorPane = source.editorPanes.find((pane) => pane.id === paneId);
  const terminalPane = (source.terminalPanes ?? []).find(
    (pane) => pane.id === paneId,
  );
  const nextSource: WorkspaceTab = {
    ...remaining,
    editorPanes: source.editorPanes.filter((pane) => pane.id !== paneId),
    terminalPanes: (source.terminalPanes ?? []).filter(
      (pane) => pane.id !== paneId,
    ),
  };
  const withoutDetached = tabs.map((tab, index) =>
    index === sourceIndex ? nextSource : tab,
  );
  const targetIndex = withoutDetached.findIndex(
    (tab) => tab.id === targetTabId,
  );
  if (targetIndex < 0) return null;

  const insertAt = targetIndex + (position === "after" ? 1 : 0);
  const keepsSourceGroup =
    !!source.groupId &&
    (withoutDetached[insertAt - 1]?.groupId === source.groupId ||
      withoutDetached[insertAt]?.groupId === source.groupId);
  const detached: WorkspaceTab = {
    kind: "session",
    id: createTabId(),
    layout: leaf(paneId),
    focusedId: paneId,
    editorPanes: editorPane ? [editorPane] : [],
    terminalPanes: terminalPane ? [terminalPane] : [],
    diffOpen: false,
    diffFocused: false,
    ...(keepsSourceGroup ? { groupId: source.groupId } : {}),
  };
  const nextTabs = withoutDetached.slice();
  nextTabs.splice(insertAt, 0, detached);
  return { tabs: nextTabs, activeTabId: detached.id, focusedId: paneId };
}

export function isGroupableProject(
  project: string | null,
): project is string {
  return !!project && project !== "~";
}

export function replaceGroupInTabOrder(
  allIds: string[],
  startIndex: number,
  length: number,
  newGroupIds: string[],
): string[] {
  const next = allIds.slice();
  next.splice(startIndex, length, ...newGroupIds);
  return next;
}
