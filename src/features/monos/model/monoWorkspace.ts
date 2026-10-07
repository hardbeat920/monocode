import { newSession, type Session } from "../../sessions/model/session";
import {
  closeLeaf,
  leafIds,
  newTab,
  type WorkspaceTab,
} from "../../workspace/model/layout";
import { workspaceTabCwd } from "../../workspace/model/workspaceTabGroups";
import { sameProjectPath } from "../../projects/model/recents";
import {
  findMono,
  isMonoSession,
  monoForSession,
  saveMonoSessionId,
} from "./mono";
import { forgetAgentContext } from "./monoFiles";
import { forgetMonoRotation } from "./monoRotation";

const openingMonos = new Map<string, Promise<Session | undefined>>();

/**
 * Load the Mono's conversation without opening or replacing a workspace tab.
 * A new one starts in the home folder: no single project is its own.
 */
export function ensureMonoSession(
  monoId: string,
  host: {
    home(): Promise<string>;
    load(id: string): Promise<Session | null | undefined>;
    create(cwd: string): Session;
    add(session: Session): void;
  },
): Promise<Session | undefined> {
  const pending = openingMonos.get(monoId);
  if (pending) return pending;
  const opening = loadMonoSession(monoId, host).finally(() => {
    if (openingMonos.get(monoId) === opening) openingMonos.delete(monoId);
  });
  openingMonos.set(monoId, opening);
  return opening;
}

async function loadMonoSession(
  monoId: string,
  host: Parameters<typeof ensureMonoSession>[1],
): Promise<Session | undefined> {
  const mono = findMono(monoId);
  if (!mono) return undefined;
  if (mono.sessionId) {
    const existing = await host.load(mono.sessionId);
    if (existing) return existing;
  }
  const home = await host.home();
  // Removed while the home folder was looked up.
  if (!findMono(monoId)) return undefined;
  const session = host.create(home);
  saveMonoSessionId(monoId, session.id);
  host.add(session);
  return session;
}

/** Reset only the Mono's chat, keeping its identity, habits and memory. */
export async function resetMonoSession(
  current: Session,
  host: {
    stop(id: string): Promise<Session | undefined>;
    remove(session: Session): Promise<void>;
    replace(session: Session): void;
  },
): Promise<Session> {
  const monoId = monoForSession(current.id)?.id;
  const stopped = (await host.stop(current.id)) ?? current;
  const fresh = newSession(
    stopped.harness,
    stopped.cwd,
    stopped.model,
    stopped.runtimeMode,
    stopped.modelSettings,
  );
  // A failed deletion must leave the original conversation selected.
  await host.remove(stopped);
  forgetAgentContext(current.id);
  forgetMonoRotation(current.id);
  if (monoId) saveMonoSessionId(monoId, fresh.id);
  host.replace(fresh);
  return fresh;
}

/** Migrate the earlier agent tabs into a separate view, keeping ordinary panes. */
export function detachMonoTabs(
  tabs: WorkspaceTab[],
  sessions: Session[],
  activeTabId: string,
  fallbackCwd: string,
  createSession: (cwd: string) => Session,
) {
  let changed = false;
  const removedProjects = new Set<string>();
  const activeTab = tabs.find((tab) => tab.id === activeTabId);
  const agentViewId =
    activeTab && isMonoSession(activeTab.focusedId)
      ? activeTab.focusedId
      : undefined;
  const regularTabs: WorkspaceTab[] = [];
  for (const tab of tabs) {
    const agentIds = leafIds(tab.layout).filter(isMonoSession);
    if (agentIds.length === 0) {
      regularTabs.push(tab);
      continue;
    }
    changed = true;
    const cwd = workspaceTabCwd(tab, sessions) ?? fallbackCwd;
    let remaining: WorkspaceTab | null = tab;
    for (const id of agentIds) {
      if (remaining) remaining = closeLeaf(remaining, id);
    }
    if (remaining) regularTabs.push(remaining);
    else removedProjects.add(cwd);
  }
  if (!changed) return undefined;

  const addedSessions: Session[] = [];
  for (const cwd of removedProjects) {
    if (
      regularTabs.some((tab) => {
        const project = workspaceTabCwd(tab, sessions);
        return project && sameProjectPath(project, cwd);
      })
    )
      continue;
    const session = createSession(cwd);
    addedSessions.push(session);
    regularTabs.push(newTab(session.id));
  }
  const activeCwd = activeTab
    ? (workspaceTabCwd(activeTab, sessions) ?? fallbackCwd)
    : fallbackCwd;
  const allSessions = [...sessions, ...addedSessions];
  const nextActive =
    regularTabs.find((tab) => tab.id === activeTabId) ??
    regularTabs.find((tab) => {
      const cwd = workspaceTabCwd(tab, allSessions);
      return cwd && sameProjectPath(cwd, activeCwd);
    }) ??
    regularTabs[0];
  return {
    tabs: regularTabs,
    addedSessions,
    activeTabId: nextActive.id,
    agentViewId,
  };
}
