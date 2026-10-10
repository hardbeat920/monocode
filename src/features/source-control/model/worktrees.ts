import { invoke } from "@tauri-apps/api/core";
import { appendReadyHandoff, buildDeterministicHandoff } from "../../sessions/model/handoff";
import { invokeWorkspace, notifyGitChanged } from "../../../platform/tauri/fs";
import { isFilesystemTab, type FilePaneTab } from "../../workspace/model/layout";
import { isEqualOrInside, pathKey } from "../../../shared/lib/paths";
import { isBlankSession } from "../../projects/model/projectReturn";
import { newSession, sessionWorkCwd, type Session } from "../../sessions/model/session";

export type Worktree = {
  path: string;
  branch: string | null;
  head: string;
  isMain: boolean;
  locked: boolean;
  prunable: boolean;
  missing: boolean;
  dirty: boolean | null;
  unpushed: number | null;
  sessionIds: string[];
};
export type Worktrees = { worktrees: Worktree[]; defaultRoot: string };

export const listWorktrees = (cwd: string) =>
  invokeWorkspace<Worktrees>("git_worktrees", { cwd });

export async function createWorktree(
  cwd: string,
  branch: string,
  base: string,
  existing: boolean,
) {
  const tree = await invoke<Worktree>("git_worktree_create", {
    cwd,
    branch,
    base,
    existing,
  });
  notifyGitChanged();
  return tree;
}

export async function createOrchestrationWorktree(
  cwd: string,
  branch: string,
) {
  const tree = await invoke<Worktree>("git_orchestration_worktree_create", {
    cwd,
    branch,
  });
  notifyGitChanged();
  return tree;
}

export async function renameWorktreeBranch(
  cwd: string,
  path: string,
  branch: string,
) {
  const tree = await invoke<Worktree>("git_worktree_rename_branch", {
    cwd,
    path,
    branch,
  });
  notifyGitChanged();
  return tree;
}

export function temporaryWorktreeBranchName(
  id: string = crypto.randomUUID(),
): string {
  const token = id
    .replace(/[^a-zA-Z0-9]/g, "")
    .slice(0, 8)
    .toLowerCase();
  return `mc/${token || Date.now().toString(36)}`;
}

export function orchestrationWorktreeBranchName(id: string): string {
  const token = id.replace(/[^a-zA-Z0-9]/g, "").slice(0, 12).toLowerCase();
  return `mc/orch-${token || Date.now().toString(36)}`;
}

export function namedWorktreeBranch(fragment: string): string | null {
  const clean = fragment
    .trim()
    .replace(/^(?:mc|monocode)\/+/, "")
    .replace(/^\/+|\/+$/g, "");
  return clean ? `mc/${clean}` : null;
}

export async function removeWorktree(
  cwd: string,
  path: string,
  force = false,
  keepSessions = false,
) {
  const result = await invoke<{ sessionIds: string[]; projectCwd: string }>(
    "git_worktree_remove",
    { cwd, path, force, keepSessions },
  );
  notifyGitChanged();
  return result;
}

export async function removeOrchestrationWorktree(
  cwd: string,
  path: string,
) {
  const result = await invoke<{ sessionIds: string[]; projectCwd: string }>(
    "git_orchestration_worktree_remove",
    { cwd, path },
  );
  notifyGitChanged();
  return result;
}

export async function removeOrchestrationBranch(
  cwd: string,
  branch: string,
) {
  await invoke<void>("git_orchestration_branch_remove", { cwd, branch });
  notifyGitChanged();
}

/** Read-only preflight; final removal must still recheck for new blockers. */
export const checkWorktreeRemoval: RemoveWorktree = (cwd, path, force) =>
  invoke("git_worktree_check_remove", { cwd, path, force });

export function assertWorktreeFilesClosed(
  path: string,
  files: readonly FilePaneTab[],
) {
  if (
    files.some(
      (file) =>
        isEqualOrInside(file.cwd, path) ||
        (isFilesystemTab(file) && isEqualOrInside(file.path, path)),
    )
  ) {
    throw new Error(
      "Close the files and terminals open in this worktree first.",
    );
  }
}

export function worktreeSessionIds(
  tree: Worktree,
  sessions: readonly Pick<
    Session,
    "id" | "cwd" | "worktreeCwd" | "worktreeRemoved"
  >[],
) {
  const ids = new Set(tree.sessionIds);
  for (const session of sessions) {
    // Live sessions override their last saved context.
    ids.delete(session.id);
    if (
      !session.worktreeRemoved &&
      isEqualOrInside(session.worktreeCwd || session.cwd, tree.path)
    )
      ids.add(session.id);
  }
  return [...ids];
}

export const NO_BRANCH_LABEL = "No branch selected";

/** Keep the transcript and project identity while requiring a new working copy. */
export function detachSessionWorktree<T extends { cwd: string; worktreeCwd?: string }>(
  session: T,
  projectCwd: string,
  path: string,
) {
  return {
    ...session,
    cwd: isEqualOrInside(session.cwd, path) ? projectCwd : session.cwd,
    worktreeCwd: session.worktreeCwd || session.cwd,
    worktreeRemoved: true,
    branch: undefined,
    providerSessionId: undefined,
    context: undefined,
    pendingSwitch: undefined,
    pendingQuestion: undefined,
    busy: false,
    queueStatus: "paused" as const,
  };
}

/**
 * Existing working copies stay bound; removed ones can be replaced in place.
 * `move` carries a conversation to another working copy in place, e.g. when
 * the agent switched checkouts on its own.
 */
export function sessionInWorktree(
  session: Session,
  tree: Worktree,
  { move = false }: { move?: boolean } = {},
): Session {
  if (
    !session.worktreeRemoved &&
    pathKey(sessionWorkCwd(session)) === pathKey(tree.path)
  )
    return session;
  const target =
    session.worktreeRemoved && !isBlankSession(session)
      ? appendReadyHandoff(
          session,
          session.harness,
          session.harness,
          `The previous working copy was deleted. Continue this conversation in ${tree.path}. Recheck the files before making changes.\n\n${buildDeterministicHandoff(session)}`,
        )
      : move && !isBlankSession(session)
      ? appendReadyHandoff(
          session,
          session.harness,
          session.harness,
          `This conversation moved from ${sessionWorkCwd(session)} to ${tree.path}${tree.branch ? ` (branch ${tree.branch})` : ""}. Continue working there. Recheck the files before making changes.\n\n${buildDeterministicHandoff(session)}`,
        )
      : isBlankSession(session)
        ? session
        : {
            ...newSession(
              session.harness,
              session.cwd,
              session.model,
              session.runtimeMode,
              session.modelSettings,
            ),
            providerAccountId: session.providerAccountId,
          };
  return {
    ...target,
    worktreeRemoved: undefined,
    worktreeCwd:
      pathKey(tree.path) === pathKey(session.cwd) ? undefined : tree.path,
    branch: tree.branch ?? undefined,
    providerSessionId: undefined,
    context: undefined,
    pendingSwitch: undefined,
  };
}

/** Fields a working-copy switch owns; everything else follows the live session. */
const WORKTREE_SWITCH_FIELDS = [
  "worktreeRemoved",
  "worktreeCwd",
  "branch",
  "providerSessionId",
  "context",
  "pendingSwitch",
] as const;

/**
 * Re-apply a switch computed from `base` onto `live`, which may have changed
 * while the switch was saving (a draft, a rename, a pin). Blocks the switch
 * added (its handoff) go before blocks added meanwhile.
 */
export function rebaseWorktreeSwitch(
  switched: Session,
  base: Session,
  live: Session,
): Session {
  if (live === base) return switched;
  const baseIds = new Set(base.blocks.map((block) => block.id));
  const added = switched.blocks.filter((block) => !baseIds.has(block.id));
  const rebased: Session = {
    ...live,
    blocks: [
      ...live.blocks.filter((block) => baseIds.has(block.id)),
      ...added,
      ...live.blocks.filter((block) => !baseIds.has(block.id)),
    ],
  };
  for (const field of WORKTREE_SWITCH_FIELDS) {
    (rebased as Record<string, unknown>)[field] = switched[field];
  }
  return rebased;
}

/**
 * Switch a session's working copy in place. The destination is saved before
 * the old provider binding and checkpoint are released, so a failed save
 * leaves the session where it was with its recovery state intact.
 */
export async function switchSessionWorktree(
  base: Session,
  tree: Worktree,
  {
    move = false,
    persist,
    release,
    live,
  }: {
    move?: boolean;
    /** Resolves falsy when the session was not saved. */
    persist: (session: Session) => Promise<unknown>;
    /** Drop provider and checkpoint state tied to the old working copy. */
    release: () => Promise<void>;
    /** The session as it is now, after the save. */
    live: () => Session | undefined;
  },
): Promise<Session> {
  const switched = sessionInWorktree(base, tree, { move });
  if (!(await persist(switched))) {
    throw new Error(
      "The session could not be saved. It stays in its current working copy.",
    );
  }
  // The switch is saved; releasing what the old copy held is best effort.
  await release().catch(() => undefined);
  const current = live();
  return current ? rebaseWorktreeSwitch(switched, base, current) : switched;
}

export type RemoveWorktree = (
  cwd: string,
  path: string,
  force: boolean,
  keepSessions?: boolean,
) => Promise<void | { sessionIds: string[]; projectCwd: string }>;
