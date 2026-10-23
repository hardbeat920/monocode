import {
  newDefaultSession,
  type RuntimeMode,
} from "../../features/sessions/model/session";
import { worktreeFocus } from "../../features/source-control/model/worktreeFocus";
import { sameProjectPath } from "../../features/projects/model/recents";

/** Session defaults can supply permissions, but the visible project owns cwd. */
export function newWorkspaceSession(
  project: string,
  runtimeMode?: RuntimeMode,
) {
  const focus = worktreeFocus(project);
  return {
    ...newDefaultSession(project, runtimeMode),
    ...(focus?.cowId
      ? { workspaceMode: "cow" as const, cowSourceCwd: focus.path, worktreeBase: undefined }
      : focus && !sameProjectPath(focus.path, project)
        ? { worktreeCwd: focus.path, branch: focus.branch ?? undefined, workspaceMode: undefined, worktreeBase: undefined }
        : {}),
  };
}
