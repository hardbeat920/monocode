import { stopStreaming } from "../../../integrations/harness/core/apply";
import { forgetHarnessSession } from "../../../integrations/harness/core/registry";
import {
  buildDeterministicHandoff,
  completeHandoff,
  isPreparingHandoff,
  sessionChildHarnesses,
} from "./handoff";
import { flushSessionCheckpoint } from "./checkpoint";
import { isFilesystemTab, type WorkspaceTab } from "../../workspace/model/layout";
import { orchestrator } from "../../orchestration/model/orchestration";
import {
  newSession,
  type HarnessId,
  type RuntimeMode,
  type Session,
} from "./session";
import {
  deleteSession,
  setSessionArchived,
  shouldPersistSession,
  upsertSession,
  type SessionSummary,
} from "../data/sessionStore";
import {
  removeSessionFromWorkspace,
  type SessionWorkspaceRemoval,
} from "./sessionWorkspaceLifecycle";
import type { WorkspaceTabCloseScope } from "../../workspace/model/workspaceTabGroups";

type Workspace = {
  tabs: WorkspaceTab[];
  sessions: Session[];
  activeTabId: string;
  dirtyFiles: ReadonlySet<string>;
};

type ReplacementSeed = {
  harness?: HarnessId;
  cwd: string;
  model?: string;
  runtimeMode?: RuntimeMode;
  modelSettings?: Record<string, string>;
};

type WorkspaceChange =
  | { type: "stopped"; session: Session }
  | { type: "orchestrationReleased"; leadId: string }
  | {
      type: "removed";
      mode: SessionRemovalMode;
      removal: SessionWorkspaceRemoval;
      session?: Session;
      savedSummary?: SessionSummary;
    };

type SessionRemovalMode = "archive" | "delete";

type SessionRemovalOptions = {
  mode: SessionRemovalMode;
  scope?: WorkspaceTabCloseScope;
  replacement: ReplacementSeed;
  workspace: {
    snapshot(): Workspace;
    apply(change: WorkspaceChange): void;
  };
  confirm(tabs: WorkspaceTab[], mode: SessionRemovalMode): Promise<boolean>;
  stop(sessionId: string): Promise<unknown>;
};

/**
 * Own the two-phase session-removal transaction. Callers supply state and I/O
 * adapters once; the returned interface exposes only the domain operation.
 */
export function createSessionRemover(options: SessionRemovalOptions): {
  remove(sessionId: string): Promise<boolean>;
} {
  const scope = options.scope ?? "project";
  const createReplacement = (latest: Session | undefined): Session => {
    const seed = latest ?? options.replacement;
    return newSession(
      seed.harness ?? "cursor",
      seed.cwd,
      seed.model,
      seed.runtimeMode,
      seed.modelSettings,
    );
  };

  return {
    remove: (sessionId) =>
      removeSession(sessionId, scope, createReplacement, options),
  };
}

/** Run the same lifecycle for archive and delete, reading state after each wait. */
async function removeSession(
  sessionId: string,
  scope: WorkspaceTabCloseScope,
  createReplacement: (seed: Session | undefined) => Session,
  options: SessionRemovalOptions,
): Promise<boolean> {
  const initial = options.workspace.snapshot();
  const plan = removeSessionFromWorkspace({
    ...initial,
    sessionId,
    scope,
    createReplacement,
  });
  if (!(await options.confirm(plan.closedTabs, options.mode))) return false;

  if (options.mode === "delete") {
    const run = orchestrator.forSession(sessionId);
    if (run && (run.status === "active" || run.status === "paused")) {
      await orchestrator.stopRun(run.leadId);
    }
  }
  await options.stop(sessionId);
  const latest = options.workspace
    .snapshot()
    .sessions.find((session) => session.id === sessionId);
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
    options.workspace.apply({ type: "stopped", session: stopped });
  }
  const harnesses: HarnessId[] = stopped
    ? sessionChildHarnesses(stopped)
    : [options.replacement.harness ?? "cursor"];
  if (options.mode === "delete") {
    // Release native processes before deleting the record, so a following
    // worktree removal cannot race fire-and-forget cleanup.
    await Promise.all(
      harnesses.map((harness) => forgetHarnessSession(harness, sessionId)),
    );
  }

  if (stopped) await flushSessionCheckpoint(sessionId);
  let savedSummary: SessionSummary | undefined;
  if (options.mode === "delete") {
    const imagePaths = stopped?.blocks.flatMap((block) =>
      block.role === "image" && block.image ? [block.image.path] : [],
    ) ?? [];
    await orchestrator.deleteSession(sessionId, () =>
      deleteSession(sessionId, imagePaths),
    );
    options.workspace.apply({
      type: "orchestrationReleased",
      leadId: sessionId,
    });
  } else {
    if (stopped && shouldPersistSession(stopped)) {
      const saved = await upsertSession(stopped);
      if (!saved) throw new Error("The conversation could not be saved.");
      savedSummary = saved;
    }
    await setSessionArchived(sessionId, true);
  }

  const current = options.workspace.snapshot();
  const removedSession = current.sessions.find(
    (session) => session.id === sessionId,
  );
  const confirmed = new Map(plan.closedTabs.map((tab) => [tab.id, tab]));
  const removal = removeSessionFromWorkspace({
    ...current,
    sessionId,
    scope,
    createReplacement,
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
  if (options.mode === "archive") {
    for (const harness of harnesses) {
      void forgetHarnessSession(harness, sessionId);
    }
  }
  options.workspace.apply({
    type: "removed",
    mode: options.mode,
    removal,
    session: removedSession,
    savedSummary,
  });
  return true;
}
