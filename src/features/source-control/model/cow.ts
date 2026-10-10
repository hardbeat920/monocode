import { invokeWorkspace, notifyGitChanged } from "../../../platform/tauri/fs";

export type CowWorkspace = {
  id: string;
  path: string;
  sourceCwd: string;
  projectCwd: string;
  sessionId: string;
  sessionIds?: string[];
  branch?: string | null;
  head: string;
  dirty?: boolean | null;
  unpushed?: number | null;
};
export const cowCapability = (cwd: string) =>
  invokeWorkspace<{ supported: boolean; reason?: string }>("cow_capability", {
    cwd,
  });
export const listCowWorkspaces = (cwd: string) =>
  invokeWorkspace<CowWorkspace[]>("cow_list", { cwd });
export async function createCowWorkspace(
  cwd: string,
  sessionId: string,
  projectCwd = cwd,
  base?: string,
) {
  const workspace = await invokeWorkspace<CowWorkspace>("cow_create", {
    cwd,
    sessionId,
    projectCwd,
    ...(base ? { base } : {}),
  });
  notifyGitChanged();
  return workspace;
}
export async function removeCowWorkspace(
  cwd: string,
  cowId: string,
  force = false,
  keepSessions = false,
) {
  const result = await invokeWorkspace<{
    sessionIds: string[];
    projectCwd: string;
  }>("cow_remove", { cwd, cowId, force, keepSessions });
  notifyGitChanged();
  return result;
}

export const checkCowRemoval = (cwd: string, cowId: string, force = false) =>
  invokeWorkspace<void>("cow_check_remove", { cwd, cowId, force });
export async function applyCowWorkspace(
  cwd: string,
  cowId: string,
  toCwd: string,
) {
  const result = await invokeWorkspace<{
    files: string[];
    alreadyApplied: number;
  }>("cow_apply", { cwd, cowId, toCwd });
  notifyGitChanged();
  return result;
}
