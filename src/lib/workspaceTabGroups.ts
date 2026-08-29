import {
  focusedFileTab,
  leafIds,
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

export function planWorkspaceTabCloseTarget({
  tabs,
  sessions,
  closingTabId,
  scope,
}: {
  tabs: WorkspaceTab[];
  sessions: Session[];
  closingTabId: string;
  scope: WorkspaceTabCloseScope;
}): string | undefined {
  const closingIndex = tabs.findIndex((tab) => tab.id === closingTabId);
  if (closingIndex < 0) return undefined;

  const remaining = tabs.filter((tab) => tab.id !== closingTabId);
  const globalTarget = remaining[Math.max(0, closingIndex - 1)] ?? remaining[0];
  if (scope === "workspace") return globalTarget?.id;

  const closingCwd = workspaceTabCwd(tabs[closingIndex], sessions);
  if (!closingCwd) return globalTarget?.id;

  for (let index = closingIndex - 1; index >= 0; index -= 1) {
    const cwd = workspaceTabCwd(tabs[index], sessions);
    if (cwd && sameProjectPath(cwd, closingCwd)) return tabs[index].id;
  }

  for (let index = closingIndex + 1; index < tabs.length; index += 1) {
    const cwd = workspaceTabCwd(tabs[index], sessions);
    if (cwd && sameProjectPath(cwd, closingCwd)) return tabs[index].id;
  }

  return globalTarget?.id;
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
