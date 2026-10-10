import { cowCapability } from "../../source-control/model/cow";
import { loadDefaultIsolationMode } from "../../settings/model/settings";
import type { WorkspaceMode, Session } from "../../sessions/model/session";
import type { QuickLaunch } from "./quickComposer";
import {
  listWorktrees,
  type Worktree,
} from "../../source-control/model/worktrees";
import { pathKey } from "../../../shared/lib/paths";

export type QuickWorkspace = {
  cwd: string | null;
  mode: WorkspaceMode;
  base?: string;
  tree?: Worktree;
};

/** Switching projects must never carry another repository's working copy/base. */
export function workspaceForProject(
  choice: QuickWorkspace,
  cwd: string | null,
): QuickWorkspace {
  return choice.cwd === cwd
    ? choice
    : { cwd, mode: loadDefaultIsolationMode() };
}

export async function quickWorkspaceLaunch(
  choice: QuickWorkspace,
): Promise<
  Pick<QuickLaunch, "workspaceMode" | "worktreeBase" | "worktreeCwd">
> {
  if (choice.mode === "cow") {
    if (!choice.cwd)
      throw new Error("Select a project for copy-on-write isolation.");
    const capability = await cowCapability(choice.cwd);
    if (!capability.supported)
      throw new Error(
        capability.reason ||
          "Copy-on-write is unavailable. Choose another isolation mode.",
      );
    return { workspaceMode: "cow", worktreeBase: choice.base || "HEAD" };
  }
  if (choice.mode === "worktree") {
    return { workspaceMode: "worktree", worktreeBase: choice.base || "HEAD" };
  }
  if (choice.tree && choice.cwd) {
    const listed = await listWorktrees(choice.cwd);
    const tree = listed.worktrees.find(
      (tree) =>
        !tree.missing && pathKey(tree.path) === pathKey(choice.tree!.path),
    );
    if (!tree)
      throw new Error(
        "This worktree is no longer available. Select another working copy.",
      );
    return pathKey(tree.path) === pathKey(choice.cwd)
      ? { workspaceMode: "current" }
      : { worktreeCwd: tree.path };
  }
  return { workspaceMode: "current" };
}

/** Feed the same deferred worktree creation path that the main composer uses. */
export function applyQuickWorkspace(
  session: Session,
  launch: QuickLaunch,
): Session {
  if (launch.workspaceMode === "cow")
    return {
      ...session,
      workspaceMode: "cow",
      worktreeBase: launch.worktreeBase || "HEAD",
    };
  if (launch.workspaceMode === "worktree") {
    return {
      ...session,
      workspaceMode: "worktree",
      worktreeBase: launch.worktreeBase || "HEAD",
    };
  }
  if (launch.workspaceMode === "current")
    return {
      ...session,
      workspaceMode: undefined,
      worktreeBase: undefined,
      worktreeCwd: undefined,
      cowId: undefined,
      cowSourceCwd: undefined,
    };
  return launch.worktreeCwd
    ? {
        ...session,
        workspaceMode: undefined,
        worktreeBase: undefined,
        worktreeCwd: launch.worktreeCwd,
      }
    : session;
}
