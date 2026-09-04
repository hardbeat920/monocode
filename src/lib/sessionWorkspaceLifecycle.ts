import {
  closeLeaf,
  firstLeafId,
  isSessionChangesTab,
  leafIds,
  removePane,
  type WorkspaceTab,
} from "./layout";
import type { Session } from "./session";
import {
  planWorkspaceTabClose,
  type WorkspaceTabCloseScope,
} from "./workspaceTabGroups";

export type SessionWorkspaceRemoval = {
  tabs: WorkspaceTab[];
  sessions: Session[];
  activeTabId: string;
  closedTabs: WorkspaceTab[];
};

export function removeSessionFromWorkspace({
  tabs,
  sessions,
  sessionId,
  activeTabId,
  scope,
  createReplacement,
}: {
  tabs: WorkspaceTab[];
  sessions: Session[];
  sessionId: string;
  activeTabId: string;
  scope: WorkspaceTabCloseScope;
  createReplacement: (seed: Session | undefined) => Session;
}): SessionWorkspaceRemoval {
  const seed = sessions.find((session) => session.id === sessionId);
  let nextTabs = [...tabs];
  let nextSessions = sessions.filter((session) => session.id !== sessionId);
  const remainingSessionIds = new Set(
    nextSessions.map((session) => session.id),
  );
  let nextActiveTabId = activeTabId;
  const closedTabs: WorkspaceTab[] = [];

  for (const original of tabs) {
    if (!leafIds(original.layout).includes(sessionId)) continue;
    const index = nextTabs.findIndex((tab) => tab.id === original.id);
    if (index < 0) continue;

    const tab = removeSessionChanges(nextTabs[index], sessionId);
    const remainingConversations = leafIds(tab.layout).some((id) =>
      remainingSessionIds.has(id),
    );

    if (remainingConversations) {
      const next = closeLeaf(tab, sessionId);
      if (next) nextTabs[index] = next;
      continue;
    }

    closedTabs.push(original);
    const closePlan = planWorkspaceTabClose({
      tabs: nextTabs,
      sessions,
      closingTabId: tab.id,
      scope,
    });
    if (closePlan.action === "close") {
      nextTabs = nextTabs.filter((entry) => entry.id !== tab.id);
      if (tab.id === nextActiveTabId && closePlan.nextActiveTabId) {
        nextActiveTabId = closePlan.nextActiveTabId;
      }
      continue;
    }

    const replacement = createReplacement(seed);
    remainingSessionIds.add(replacement.id);
    nextSessions = [...nextSessions, replacement];
    nextTabs[index] = {
      ...tab,
      layout: { type: "leaf", id: replacement.id },
      focusedId: replacement.id,
      editorPanes: [],
      terminalPanes: [],
      diffOpen: false,
      diffFocused: false,
    };
  }

  return {
    tabs: nextTabs,
    sessions: nextSessions,
    activeTabId: nextActiveTabId,
    closedTabs,
  };
}

function removeSessionChanges(tab: WorkspaceTab, sessionId: string): WorkspaceTab {
  let layout = tab.layout;
  let focusedId = tab.focusedId;
  const editorPanes = [];

  for (const pane of tab.editorPanes) {
    const files = pane.files.filter(
      (file) => !isSessionChangesTab(file) || file.sessionChanges.sessionId !== sessionId,
    );
    if (files.length > 0) {
      editorPanes.push({
        ...pane,
        files,
        activeFileId: files.some((file) => file.id === pane.activeFileId)
          ? pane.activeFileId
          : files[0].id,
      });
      continue;
    }
    const nextLayout = removePane(layout, pane.id);
    if (!nextLayout) continue;
    layout = nextLayout;
    if (focusedId === pane.id) focusedId = firstLeafId(nextLayout);
  }

  return { ...tab, layout, focusedId, editorPanes };
}
