import { focusedFileTab, type WorkspaceTab } from "./layout";
import { pathKey } from "./paths";
import { sameProjectPath } from "./recents";
import type { Session } from "./session";
import { findTabForProject, workspaceTabCwd } from "./workspaceTabGroups";

export type ProjectReturnMemory = ReadonlyMap<string, WorkspaceTab["id"]>;

type ProjectReturnContext = {
  memory: ProjectReturnMemory;
  tabs: WorkspaceTab[];
  sessions: Session[];
  activeTabId: string;
};

export type ProjectReturnDecision =
  | { action: "keep" }
  | { action: "activate"; tabId: string }
  | { action: "reuse-blank"; sessionId: string }
  | { action: "create" };

export function isBlankSession(session: Session | undefined): boolean {
  if (!session || session.busy) return false;
  return !session.blocks.some((block) => block.role === "user");
}

export function reconcileProjectReturn({
  memory,
  tabs,
  sessions,
  activeTabId,
}: Omit<ProjectReturnContext, "sessions"> & {
  sessions: readonly Pick<Session, "id" | "cwd">[];
}): ProjectReturnMemory {
  const projects = new Map<string, string>();
  for (const tab of tabs) {
    const cwd = workspaceTabCwd(tab, sessions);
    if (cwd) projects.set(tab.id, pathKey(cwd));
  }
  const next = new Map(
    [...memory].filter(([project, tabId]) => projects.get(tabId) === project),
  );
  const activeProject = projects.get(activeTabId);
  if (activeProject) next.set(activeProject, activeTabId);
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
  const active = tabs.find((tab) => tab.id === activeTabId);
  const current =
    active && sessions.find((session) => session.id === active.focusedId);
  const currentCwd =
    current?.cwd ?? (active ? focusedFileTab(active)?.cwd : undefined);
  if (currentCwd && sameProjectPath(currentCwd, projectPath))
    return { action: "keep" };
  const remembered = tabs.find(
    (tab) => tab.id === memory.get(pathKey(projectPath)),
  );
  const rememberedCwd = remembered && workspaceTabCwd(remembered, sessions);
  if (
    remembered &&
    rememberedCwd &&
    sameProjectPath(rememberedCwd, projectPath)
  ) {
    return { action: "activate", tabId: remembered.id };
  }
  const match = findTabForProject(tabs, sessions, projectPath);
  if (match) return { action: "activate", tabId: match.id };
  return current && isBlankSession(current)
    ? { action: "reuse-blank", sessionId: current.id }
    : { action: "create" };
}
