import { stopStreaming } from "./harness/apply";
import {
  buildDeterministicHandoff,
  completeHandoff,
  isPreparingHandoff,
} from "./handoff";
import { isFilesystemTab, type WorkspaceTab } from "./layout";
import type { Session } from "./session";
import {
  removeSessionFromWorkspace,
  type SessionWorkspaceRemoval,
} from "./sessionWorkspaceLifecycle";
import type { WorkspaceTabCloseScope } from "./workspaceTabGroups";

type Workspace = {
  tabs: WorkspaceTab[];
  sessions: Session[];
  activeTabId: string;
  dirtyFiles: ReadonlySet<string>;
};

/** Run the same lifecycle for archive and delete, reading state after each wait. */
export async function runSessionRemoval(options: {
  sessionId: string;
  scope: WorkspaceTabCloseScope;
  readWorkspace: () => Workspace;
  createReplacement: (seed: Session | undefined) => Session;
  confirmClose: (tabs: WorkspaceTab[]) => Promise<boolean>;
  stop: () => Promise<void>;
  updateSession: (session: Session) => void;
  persist: (session: Session | undefined) => Promise<void>;
  commit: (removal: SessionWorkspaceRemoval) => void;
}): Promise<boolean> {
  const initial = options.readWorkspace();
  const plan = removeSessionFromWorkspace({ ...initial, ...options });
  if (!(await options.confirmClose(plan.closedTabs))) return false;

  await options.stop();
  const latest = options
    .readWorkspace()
    .sessions.find((session) => session.id === options.sessionId);
  let stopped = latest;
  if (latest) {
    stopped = latest.busy ? stopStreaming(latest) : latest;
    if (isPreparingHandoff(stopped)) {
      stopped = completeHandoff(stopped, buildDeterministicHandoff(stopped));
    }
    if (stopped.queuedMessages?.length) {
      stopped = { ...stopped, queueStatus: "paused" };
    }
    // Cancellation invalidates normal turn completion. Keep a usable stopped
    // session even when the following storage operation fails.
    options.updateSession(stopped);
  }
  await options.persist(stopped);

  const current = options.readWorkspace();
  const confirmed = new Map(plan.closedTabs.map((tab) => [tab.id, tab]));
  const removal = removeSessionFromWorkspace({
    ...current,
    ...options,
    canCloseTab: (tab) => {
      const before = confirmed.get(tab.id);
      // File/terminal panes opened or rearranged during a dialog or save were
      // never confirmed. Remove the conversation but keep those surfaces open.
      if (
        !before ||
        before.editorPanes !== tab.editorPanes ||
        before.terminalPanes !== tab.terminalPanes
      )
        return false;
      return !tab.editorPanes.some((pane) =>
        pane.files.some(
          (file) =>
            isFilesystemTab(file) &&
            current.dirtyFiles.has(file.id) &&
            !initial.dirtyFiles.has(file.id),
        ),
      );
    },
  });
  // No await between the final read and commit: unrelated streaming updates,
  // tabs, and focus changes must survive this operation.
  options.commit(removal);
  return true;
}
