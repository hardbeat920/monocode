import type { Session } from "../../sessions/model/session";
import type { HostSession } from "./protocol";
import { sameModelSettings } from "./remoteModels";
import { remotePath, type RemoteProject } from "./remoteProjects";

export type RemoteChoice = Pick<
  Session,
  "harness" | "model" | "modelSettings" | "runtimeMode"
>;

/** A remote tab's shell carries the provider choice its pane shows, the host
 * session's or the composer's, as a local tab's session does: the tab strip
 * shows it and the saved workspace restores it. The same array comes back
 * when nothing changed, so the session store does not wake the workspace. */
export function withRemoteChoice(
  sessions: Session[],
  shellId: string,
  choice: RemoteChoice,
): Session[] {
  const shell = sessions.find((entry) => entry.id === shellId);
  if (
    !shell ||
    (shell.harness === choice.harness &&
      shell.model === choice.model &&
      shell.runtimeMode === choice.runtimeMode &&
      sameModelSettings(shell.modelSettings, choice.modelSettings))
  )
    return sessions;
  return sessions.map((entry) =>
    entry === shell ? { ...shell, ...choice } : entry,
  );
}

/** Show the host's conversation in the app's ordinary session state while
 * retaining the local tab ID and the project's remote path. */
export function remoteSessionState(
  shell: Session,
  snapshot: HostSession,
  project: RemoteProject,
): Session {
  const host = snapshot.session;
  return {
    ...shell,
    ...host,
    id: shell.id,
    cwd: shell.cwd,
    worktreeCwd: host.cwd === project.cwd
      ? undefined
      : remotePath(project.environmentId, host.cwd),
  };
}
