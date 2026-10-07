import {
  newSessionForProject,
  type Session,
} from "../../sessions/model/session";
import { newTab, type WorkspaceTab } from "../../workspace/model/layout";
import { looksLikeProject, normalizeProjectPath } from "./recents";
import { planProjectReturn, type ProjectReturnMemory } from "./projectReturn";

export type ProjectOpenStep =
  | { action: "keep"; path: string }
  | { action: "activate"; path: string; tabId: string; paneId?: string }
  | { action: "reuse-blank"; path: string; sessionId: string }
  | {
      action: "create";
      path: string;
      session: Session;
      tab: WorkspaceTab;
      /** Tab the new one sits beside: the one created before it in this run. */
      besideTabId?: string;
    };

/**
 * What opening a set of folders should do, decided in one pass.
 *
 * Every step is planned against the workspace snapshot *plus* what earlier
 * steps in the same run already produced, so the caller can commit the whole
 * run in a single state transition. Deciding per folder from refs instead would
 * plan each against state React has not rendered yet: the same blank session
 * reused twice, duplicate tabs for a project already open, and tabs all
 * inserted beside the same anchor.
 */
export function planProjectOpenRun({
  memory,
  tabs,
  sessions,
  activeTabId,
  paths,
}: {
  memory: ProjectReturnMemory;
  tabs: WorkspaceTab[];
  sessions: Session[];
  activeTabId: string;
  paths: readonly string[];
}): ProjectOpenStep[] {
  const activeTab = tabs.find((tab) => tab.id === activeTabId);
  const seed =
    sessions.find((session) => session.id === activeTab?.focusedId) ??
    sessions[0];

  const steps: ProjectOpenStep[] = [];
  let openTabs = tabs;
  let openSessions = sessions;
  let besideTabId: string | undefined = activeTabId;
  let blankReused = false;

  for (const path of paths) {
    const normalized = normalizeProjectPath(path);
    if (!looksLikeProject(normalized)) continue;

    const decision = planProjectReturn({
      memory,
      tabs: openTabs,
      sessions: openSessions,
      activeTabId,
      projectPath: normalized,
    });
    if (decision.action === "keep") {
      steps.push({ action: "keep", path: normalized });
      continue;
    }
    if (decision.action === "activate") {
      steps.push({
        action: "activate",
        path: normalized,
        tabId: decision.tabId,
        paneId: decision.paneId,
      });
      continue;
    }
    // Only the first folder can take the blank session; the rest would
    // otherwise overwrite it in turn.
    if (decision.action === "reuse-blank" && !blankReused) {
      blankReused = true;
      steps.push({
        action: "reuse-blank",
        path: normalized,
        sessionId: decision.sessionId,
      });
      openSessions = openSessions.map((session) =>
        session.id === decision.sessionId
          ? { ...session, cwd: normalized }
          : session,
      );
      continue;
    }

    const session = newSessionForProject(seed, normalized);
    const tab = newTab(session.id);
    steps.push({
      action: "create",
      path: normalized,
      session,
      tab,
      besideTabId,
    });
    openSessions = [...openSessions, session];
    openTabs = [...openTabs, tab];
    besideTabId = tab.id;
  }

  return steps;
}
