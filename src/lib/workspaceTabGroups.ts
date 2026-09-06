import {
  closeLeaf,
  focusedFileTab,
  leaf,
  leafIds,
  placePane,
  replaceLeafId,
  type PaneEdge,
  type WorkspaceTab,
} from "./layout";
import { projectName } from "./paths";
import { sameProjectPath } from "./recents";
import type { Session } from "./session";

export function workspaceTabCwd(
  tab: WorkspaceTab,
  sessions: Session[],
): string | null {
  for (const id of leafIds(tab.layout)) {
    const session = sessions.find((entry) => entry.id === id);
    if (session?.cwd && session.cwd !== "~") return session.cwd;
  }

  const file = focusedFileTab(tab);
  if (file?.cwd && file.cwd !== "~") return file.cwd;

  return null;
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
