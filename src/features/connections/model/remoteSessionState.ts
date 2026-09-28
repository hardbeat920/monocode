import type { Session } from "../../sessions/model/session";
import type { HostSession } from "./protocol";
import { remotePath, type RemoteProject } from "./remoteProjects";

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
