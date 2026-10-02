import { sameProjectPath } from "../../projects/model/recents";
import {
  persistFingerprint,
  shouldPersistSession,
  upsertSession,
} from "../../sessions/data/sessionStore";
import { isPreparingHandoff } from "../../sessions/model/handoff";
import { hasUnsavedComposerDraft } from "../../sessions/model/draftCache";
import {
  newSessionLike,
  sessionNeedsInput,
  type Session,
} from "../../sessions/model/session";
import {
  closeLeaf,
  leafIds,
  resetTabToSession,
  type WorkspaceTab,
} from "../../workspace/model/layout";
import { planWorkspaceTabClose } from "../../workspace/model/workspaceTabGroups";

type Workspace = {
  sessions: Session[];
  tabs: WorkspaceTab[];
  activeTabId: string;
};

/** Persist first, then synchronously remove only this session's view leaves. */
export async function closeAgentSession(
  source: Session,
  id: string,
  workspace: {
    snapshot(): Workspace;
    unavailable(id: string): boolean;
    worktreeOf(tab: WorkspaceTab): string | null;
    apply(next: Workspace): void;
  },
): Promise<{ closed: boolean }> {
  const targetIn = (state: Workspace) => {
    if (id === source.id) throw new Error("Cannot close the calling session");
    const target = state.sessions.find((session) => session.id === id);
    if (
      workspace.unavailable(id) ||
      (target &&
        (target.inboxAsk ||
          target.orchestrationLeadId ||
          !sameProjectPath(target.cwd, source.cwd)))
    )
      throw new Error("Session is unavailable in this project");
    if (
      target &&
      (target.busy ||
        target.worktreePreparing ||
        sessionNeedsInput(target) ||
        target.backgroundTasks?.length ||
        target.queuedMessages?.length ||
        isPreparingHandoff(target) ||
        target.blocks.some((block) =>
          block.btwThreads?.some((thread) => thread.status === "running"),
        ))
    )
      throw new Error("Session is busy; try again when it finishes");
    return target;
  };
  const isOpen = (state: Workspace) =>
    state.tabs.some((tab) => leafIds(tab.layout).includes(id));
  const checkDraft = (target: Session) => {
    if (
      hasUnsavedComposerDraft(id) ||
      (target.composerSeed && !target.blocks.length) ||
      target.inboxCard ||
      target.noteCard ||
      target.handoffCard
    )
      throw new Error(
        "Session has unsaved composer text or attachments; save with /draft in MonoCode (or finish/remove side-question drafts), then retry",
      );
  };
  const initial = workspace.snapshot();
  const target = targetIn(initial);
  if (!isOpen(initial)) return { closed: false };
  if (!target) throw new Error("Session view is still loading; try again");
  checkDraft(target);
  const fingerprint = persistFingerprint(target);
  if (shouldPersistSession(target) && !(await upsertSession(target)))
    throw new Error("Session could not be saved; its view was left open");

  // Saving crosses IPC. Recheck both the session and the layout before closing;
  // a user may have typed, started a turn, moved a pane, or switched worktrees.
  const current = workspace.snapshot();
  const latest = targetIn(current);
  if (!isOpen(current)) return { closed: false };
  if (!latest || persistFingerprint(latest) !== fingerprint)
    throw new Error(
      "Session changed while saving; its view was left open, try again",
    );
  checkDraft(latest);
  let tabs = [...current.tabs];
  let sessions = current.sessions;
  let activeTabId = current.activeTabId;
  for (const tab of current.tabs) {
    if (!leafIds(tab.layout).includes(id)) continue;
    const next = closeLeaf(tab, id);
    if (next) {
      tabs = tabs.map((entry) => (entry.id === tab.id ? next : entry));
      continue;
    }
    const plan = planWorkspaceTabClose({
      tabs,
      sessions,
      closingTabId: tab.id,
      scope: "project",
      worktreeOf: workspace.worktreeOf,
    });
    if (plan.action === "close") {
      tabs = tabs.filter((entry) => entry.id !== tab.id);
      if (activeTabId === tab.id) activeTabId = plan.nextActiveTabId!;
    } else {
      const replacement = newSessionLike(latest, latest.cwd);
      replacement.worktreeCwd = latest.worktreeCwd;
      sessions = [...sessions, replacement];
      tabs = tabs.map((entry) =>
        entry.id === tab.id ? resetTabToSession(entry, replacement.id) : entry,
      );
    }
  }
  // Keep the transcript attached until the ordinary idle-detach lifecycle runs.
  workspace.apply({ tabs, sessions, activeTabId });
  return { closed: true };
}
