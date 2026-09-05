import {
  closeLeaf,
  firstLeafId,
  isSessionChangesTab,
  leafIds,
  removePane,
  type EditorPane,
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
  canCloseTab = () => true,
}: {
  tabs: WorkspaceTab[];
  sessions: Session[];
  sessionId: string;
  activeTabId: string;
  scope: WorkspaceTabCloseScope;
  createReplacement: (seed: Session | undefined) => Session;
  /** Preserve file panes changed while an asynchronous close was pending. */
  canCloseTab?: (tab: WorkspaceTab) => boolean;
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
    const hadSession = leafIds(original.layout).includes(sessionId);
    const index = nextTabs.findIndex((tab) => tab.id === original.id);
    if (index < 0) continue;

    const cleaned = removeSessionDocuments(nextTabs[index], sessionId);
    if (!hadSession && cleaned) {
      nextTabs[index] = cleaned;
      continue;
    }
    const tab = cleaned ?? original;
    const remainingConversations = leafIds(tab.layout).some((id) =>
      remainingSessionIds.has(id),
    );

    if (remainingConversations || (cleaned && !canCloseTab(original))) {
      const next = closeLeaf(tab, sessionId);
      if (next) nextTabs[index] = next;
      if (next) continue;
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

function removeSessionDocuments(
  tab: WorkspaceTab,
  sessionId: string,
): WorkspaceTab | null {
  if (
    !tab.editorPanes.some((pane) =>
      pane.files.some(
        (file) =>
          file.plan?.sessionId === sessionId ||
          (isSessionChangesTab(file) &&
            file.sessionChanges.sessionId === sessionId),
      ),
    )
  )
    return tab;
  let layout = tab.layout;
  let focusedId = tab.focusedId;
  const editorPanes: EditorPane[] = [];

  for (const pane of tab.editorPanes) {
    const files = pane.files.filter(
      (file) =>
        file.plan?.sessionId !== sessionId &&
        (!isSessionChangesTab(file) ||
          file.sessionChanges.sessionId !== sessionId),
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
    if (!nextLayout) return null;
    layout = nextLayout;
    if (focusedId === pane.id) focusedId = firstLeafId(nextLayout);
  }

  return { ...tab, layout, focusedId, editorPanes };
}
