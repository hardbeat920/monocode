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

/**
 * Close all views of a project-authorized target after saving its transcript.
 * Reject active work, unsaved input, failed saves, and changes during the save;
 * no layout is applied on those errors. Return false if no view remains open.
 * Retain the session for ordinary idle detachment, which may dispose of its
 * provider process; this function does not cancel a turn or delete history.
 */
export async function closeAgentSession(
  source: Session,
  id: string,
  workspace: {
    /** Read current refs so post-save validation does not use a stale render. */
    snapshot(): Workspace;
    /** Report removal, worktree switching, or orchestration ownership. */
    unavailable(id: string): boolean;
    /** Resolve the tab's worktree for the existing close/fallback policy. */
    worktreeOf(tab: WorkspaceTab): string | null;
    /** Commit the planned layout synchronously, without another async gap. */
    apply(next: Workspace): void;
  },
): Promise<{ closed: boolean }> {
  /** Revalidate access and activity in this snapshot; a detached target is absent. */
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
  /** Check every tab, including shared layouts, for a target session leaf. */
  const isOpen = (state: Workspace) =>
    state.tabs.some((tab) => leafIds(tab.layout).includes(id));
  /** Refuse input held only in UI memory; persisted draft blocks may close safely. */
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
