import { leafIds, type WorkspaceTab } from "./layout";
import type { Session } from "./session";
import { pathKey } from "./paths";
import { sameProjectPath } from "./recents";

export type ProjectReturnMemory = ReadonlyMap<string, string>;

type ProjectReturnContext = {
  memory: ProjectReturnMemory;
  tabs: WorkspaceTab[];
  sessions: Session[];
  activeTabId: string;
};

type PaneProject = Map<string, string>;

export type ProjectReturnDecision =
  | { action: "keep" }
  | { action: "activate"; tabId: string; paneId?: string }
  | { action: "reuse-blank"; sessionId: string }
  | { action: "create" };

export function isBlankSession(session: Session | undefined): boolean {
  if (!session || session.busy) return false;
  return !session.blocks.some((block) => block.role === "user");
}

function paneProjects(
  tabs: readonly WorkspaceTab[],
  sessions: readonly Pick<Session, "id" | "cwd">[],
): PaneProject {
  const cwdBySession = new Map(
    sessions.map((session) => [session.id, session.cwd]),
  );
  const result = new Map<string, string>();

  for (const tab of tabs) {
    for (const paneId of leafIds(tab.layout)) {
      const cwd = cwdBySession.get(paneId);
      if (cwd && cwd !== "~") {
        result.set(paneId, pathKey(cwd));
      }
    }

    for (const pane of tab.editorPanes) {
      const active = pane.files.find((file) => file.id === pane.activeFileId);
      if (active && active.cwd !== "~") {
        result.set(pane.id, pathKey(active.cwd));
      }
    }

    for (const pane of tab.terminalPanes ?? []) {
      const active = pane.files.find((file) => file.id === pane.activeFileId);
      if (active && active.cwd !== "~") {
        result.set(pane.id, pathKey(active.cwd));
      }
    }
  }

  return result;
}

function tabFocusedPaneById(tabs: readonly WorkspaceTab[]): Map<string, string> {
  return new Map(tabs.map((tab) => [tab.id, tab.focusedId]));
}

function paneBelongsToProject(
  paneId: string,
  target: string,
  paneById: PaneProject,
): boolean {
  const project = paneById.get(paneId);
  return !!project && sameProjectPath(project, target);
}

function paneForProjectInTab(
  tab: WorkspaceTab,
  target: string,
  paneById: PaneProject,
): string | undefined {
  for (const paneId of leafIds(tab.layout)) {
    if (paneById.get(paneId) === target) return paneId;
  }
  for (const pane of tab.editorPanes) {
    if (paneById.get(pane.id) === target) return pane.id;
  }
  for (const pane of tab.terminalPanes ?? []) {
    if (paneById.get(pane.id) === target) return pane.id;
  }
  return undefined;
}

function tabContainsPane(tab: WorkspaceTab, paneId: string): boolean {
  return (
    leafIds(tab.layout).includes(paneId) ||
    tab.editorPanes.some((pane) => pane.id === paneId) ||
    (tab.terminalPanes ?? []).some((pane) => pane.id === paneId)
  );
}

export function reconcileProjectReturn({
  memory,
  tabs,
  sessions,
  activeTabId,
}: Omit<ProjectReturnContext, "sessions"> & {
  sessions: readonly Pick<Session, "id" | "cwd">[];
}): ProjectReturnMemory {
  const activeTab = tabs.find((tab) => tab.id === activeTabId);
  const byPane = paneProjects(tabs, sessions);
  const byTab = tabFocusedPaneById(tabs);

  const next = new Map<string, string>();
  for (const [project, saved] of memory) {
    const asPane = byPane.get(saved);
    if (asPane === project) {
      next.set(project, saved);
      continue;
    }

    const focused = byTab.get(saved);
    if (!focused) continue;
    if (paneBelongsToProject(focused, project, byPane)) {
      next.set(project, focused);
    }
  }

  const activePaneId = activeTab?.focusedId;
  const activeProject = activePaneId ? byPane.get(activePaneId) : undefined;
  if (activeProject && activePaneId) next.set(activeProject, activePaneId);

  if (
    next.size === memory.size &&
    [...next].every(([project, id]) => memory.get(project) === id)
  )
    return memory;
  return next;
}

export function planProjectReturn({
  memory,
  tabs,
  sessions,
  activeTabId,
  projectPath,
}: ProjectReturnContext & { projectPath: string }): ProjectReturnDecision {
  const target = pathKey(projectPath);
  const byPane = paneProjects(tabs, sessions);
  const active = tabs.find((tab) => tab.id === activeTabId);

  if (active?.focusedId && paneBelongsToProject(active.focusedId, target, byPane)) {
    return { action: "keep" };
  }

  const remembered = memory.get(target);
  if (remembered) {
    const rememberedTab = tabs.find((tab) => tabContainsPane(tab, remembered));
    if (
      rememberedTab &&
      paneBelongsToProject(remembered, target, byPane)
    ) {
      return {
        action: "activate",
        tabId: rememberedTab.id,
        paneId: remembered,
      };
    }
  }

  for (const tab of tabs) {
    const paneId = paneForProjectInTab(tab, target, byPane);
    if (!paneId) continue;
    return { action: "activate", tabId: tab.id, paneId };
  }

  const current =
    active?.focusedId &&
    sessions.find((session) => session.id === active.focusedId);
  return current && isBlankSession(current)
    ? { action: "reuse-blank", sessionId: current.id }
    : { action: "create" };
}
