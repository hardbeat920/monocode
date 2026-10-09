import {
  isRemoteProjectPath,
  REMOTE_PROJECT_PREFIX,
} from "../../projects/model/recents";
import type { HostProject } from "./protocol";

/** A rail project whose folder lives on another machine. */
export type RemoteProject = {
  key: string;
  environmentId: string;
  /** The host's ID for this folder. */
  projectId: string;
  /** The folder's path on the host. */
  cwd: string;
};

const KEY = "monocode.remote-projects.v2";
export const REMOTE_PROJECTS_CHANGED = "monocode:remote-projects-changed";

const slashed = (path: string) => path.replace(/\\/g, "/");

export function remoteProjectKey(environmentId: string, cwd: string): string {
  return remotePath(environmentId, slashed(cwd).replace(/\/+$/, ""));
}

/** How this app addresses a path on another machine: under that machine's
 * `remote://<environment>/` root, so every file view can tell where it lives. */
export function remotePath(environmentId: string, hostPath: string): string {
  const path = slashed(hostPath);
  // Keep the second leading slash of a Windows UNC path.
  return `${REMOTE_PROJECT_PREFIX}${environmentId}/${path.startsWith("//") ? path.slice(1) : path.replace(/^\/+/, "")}`;
}

/** The machine and host path behind a `remote://` path. */
export function parseRemotePath(
  path: string,
): { environmentId: string; hostPath: string } | undefined {
  if (!isRemoteProjectPath(path)) return undefined;
  const rest = slashed(path).slice(REMOTE_PROJECT_PREFIX.length);
  const slash = rest.indexOf("/");
  if (slash <= 0) return undefined;
  const hostPath = rest.slice(slash + 1);
  return {
    environmentId: rest.slice(0, slash),
    // Windows hosts keep their drive letter and UNC prefix; POSIX paths regain their root.
    hostPath: /^[A-Za-z]:(\/|$)/.test(hostPath) ? hostPath : `/${hostPath}`,
  };
}

function readAll(): Record<string, RemoteProject> {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return value && typeof value === "object"
      ? (value as Record<string, RemoteProject>)
      : {};
  } catch {
    return {};
  }
}

/** The remote path git operations should run in for the active session: the
 * session's own work path when it differs from the project root (a linked
 * worktree), not the terminal tab's cwd, which follows the shell a session
 * was launched from rather than the repo its changes live in. */
export function remoteSessionGitCwd(
  project: Pick<RemoteProject, "environmentId" | "cwd">,
  gitCwd: string | undefined,
  projectRoot: string,
): string {
  if (gitCwd && isRemoteProjectPath(gitCwd)) return gitCwd;
  return remotePath(
    project.environmentId,
    gitCwd && gitCwd !== projectRoot ? gitCwd : project.cwd,
  );
}

export function remoteProjectFor(path: string): RemoteProject | undefined {
  if (!isRemoteProjectPath(path)) return undefined;
  const key = slashed(path).replace(/\/+$/, "");
  const projects = readAll();
  return projects[key] ?? Object.values(projects).find(
    (project) => remoteProjectKey(project.environmentId, project.cwd) === key,
  );
}

export function rememberRemoteProject(
  environmentId: string,
  project: HostProject,
): RemoteProject {
  const remote: RemoteProject = {
    key: remoteProjectKey(environmentId, project.cwd),
    environmentId,
    projectId: project.id,
    cwd: project.cwd,
  };
  try {
    localStorage.setItem(
      KEY,
      JSON.stringify({ ...readAll(), [remote.key]: remote }),
    );
  } catch {
    /* the rail entry still works for this session */
  }
  window.dispatchEvent(new Event(REMOTE_PROJECTS_CHANGED));
  return remote;
}

export function remoteProjectsOn(environmentId: string): RemoteProject[] {
  return Object.values(readAll()).filter(
    (project) => project.environmentId === environmentId,
  );
}
