import { composerSeedForAddToChat, type AddToChatMode } from "./quoteDraft";
import { newDefaultSession, newSessionLike, type Session } from "./session";
import {
  focusedFileTab,
  leafIds,
  newTab,
  type WorkspaceTab,
} from "../../workspace/model/layout";
import {
  focusedWorkspaceTabCwd,
  openAddToChatSessionPane,
} from "../../workspace/model/workspaceTabGroups";

export type AddChatToWorkspaceResult = {
  /** Updated sessions array including the new chat. */
  sessions: Session[];
  /** Updated tabs array including the tab hosting the new chat. */
  tabs: WorkspaceTab[];
  /** Id of the tab that should become active. */
  activeTabId: string;
  /** Id of the new chat session. */
  sessionId: string;
};

/**
 * Handle an add-to-chat request end to end.
 *
 * Normal path: split the request into the active (or first) tab beside a
 * file-only pane. A mounted session pane owns add-to-chat instead, so the
 * flow bails (null) when the target tab already shows a session.
 *
 * Zero-tab path (issue #311): every workspace tab is closed, so build one
 * session seeded with the quoted text and open it as the replacement tab.
 * PR #325 review: the helper's tab already hosts the new session, so it is
 * used directly — no second split beside itself, no duplicate panes. The
 * cwd is always the project directory (never another project's session
 * cwd), and the new session doubles as the harness/model/settings donor so
 * exactly one conversation is created.
 */
export function applyAddToChatRequest({
  sessions,
  tabs,
  activeTabId,
  projectCwd,
  fallbackCwd,
  defaultRuntimeMode,
  text,
  mode = "quote",
}: {
  sessions: readonly Session[];
  tabs: readonly WorkspaceTab[];
  activeTabId?: string | null;
  projectCwd: string;
  /** cwd fallback for the normal path (App's sessionDefaults?.cwd). */
  fallbackCwd?: string;
  defaultRuntimeMode?: Session["runtimeMode"];
  text: string;
  mode?: AddToChatMode;
}): AddChatToWorkspaceResult | null {
  const composerSeed = composerSeedForAddToChat(text, mode);
  if (!composerSeed) return null;

  let currentSessions = sessions;
  let currentTabs = tabs;
  let tab =
    currentTabs.find((entry) => entry.id === activeTabId) ?? currentTabs[0];
  let createdSession: Session | undefined;

  if (!tab) {
    // Zero-tab fallback: seed one replacement chat from the last known
    // session's harness/model/settings, but never its cwd.
    const donor = sessions[sessions.length - 1];
    createdSession = {
      ...newSessionLike(donor, projectCwd),
      composerSeed,
    };
    tab = newTab(createdSession.id);
    currentSessions = [...currentSessions, createdSession];
    currentTabs = [...currentTabs, tab];
  }

  const mountedSessionIds = new Set(
    currentSessions.map((session) => session.id),
  );
  // The fallback tab wraps the just-seeded session, so its only leaf is
  // "mounted" by construction; the guard below must not reject it.
  if (
    !createdSession &&
    leafIds(tab.layout).some((id) => mountedSessionIds.has(id))
  ) {
    return null;
  }

  const cwd =
    focusedWorkspaceTabCwd(tab, currentSessions) ??
    fallbackCwd ??
    projectCwd;
  const file = focusedFileTab(tab);
  const session = createdSession ?? {
    ...newDefaultSession(cwd, defaultRuntimeMode),
    ...(file?.projectCwd ? { worktreeCwd: file.cwd } : {}),
    composerSeed,
  };

  let nextTab: WorkspaceTab;
  if (createdSession) {
    // The fallback tab already hosts the new session; splitting it beside
    // itself would duplicate the pane (PR #325 review).
    nextTab = tab;
  } else {
    const openedTab = openAddToChatSessionPane({
      tab,
      sessions,
      sessionId: session.id,
    });
    // A mounted session pane owns the normal add-to-chat path.
    if (!openedTab) return null;
    nextTab = openedTab;
  }

  return {
    sessions: createdSession
      ? [...currentSessions]
      : [...currentSessions, session],
    tabs: createdSession
      ? [...currentTabs]
      : currentTabs.map((entry) => (entry.id === tab!.id ? nextTab : entry)),
    activeTabId: tab.id,
    sessionId: session.id,
  };
}
