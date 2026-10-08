import {
  sessionDraftBlock,
  type Session,
} from "../../sessions/model/session";
import type { AppSessionListing } from "../../agent-app/model/agentApp";
import {
  cachedRemoteSessionSummary,
  loadRemoteSession,
  remoteMachineFor,
  remoteRequest,
  remoteSessionFor,
} from "./connections";
import type { HostSessionSummary } from "./protocol";
import { remoteProjectFor } from "./remoteProjects";
import { remoteSessionState } from "./remoteSessionState";

/** A remote tab's conversation, read from its host. The local tab holds only
 * an empty shell until its pane mounts and syncs. */
export async function readRemoteShell(shell: Session): Promise<Session> {
  const project = remoteProjectFor(shell.cwd);
  if (!project)
    throw new Error("This remote project's machine details are missing");
  const sessionId = remoteSessionFor(shell.id);
  if (!sessionId) return shell;
  const machine = await remoteMachineFor(project.environmentId);
  if (!machine)
    throw new Error(
      "The machine for this remote project isn't connected on this computer",
    );
  const snapshot = await loadRemoteSession(machine.id, sessionId);
  if (snapshot.projectId !== project.projectId)
    throw new Error("This session belongs to a different host project");
  return remoteSessionState(shell, snapshot, project);
}

/** List entries for a remote project's tabs, with the host's titles and run
 * state. Falls back to the last list read while the machine is unreachable. */
export async function remoteShellListings(
  cwd: string,
  shells: Session[],
): Promise<AppSessionListing[]> {
  const project = remoteProjectFor(cwd);
  let summaries: HostSessionSummary[] | undefined;
  if (project && shells.length)
    try {
      const machine = await remoteMachineFor(project.environmentId);
      if (machine)
        summaries = await remoteRequest<HostSessionSummary[]>(
          machine.id,
          "sessions.list",
          { projectId: project.projectId },
        );
    } catch {
      summaries = undefined;
    }
  return shells.map((shell) => {
    const sessionId = remoteSessionFor(shell.id);
    const summary = !sessionId
      ? undefined
      : summaries
        ? summaries.find((entry) => entry.id === sessionId)
        : cachedRemoteSessionSummary(shell.cwd, sessionId);
    return {
      id: shell.id,
      title: summary?.title || shell.title,
      harness: summary?.harness ?? shell.harness,
      model: summary?.model ?? shell.model,
      busy: summary ? summary.status === "running" : !!shell.busy,
      hasDraft: summary?.draft ?? !!sessionDraftBlock(shell),
    };
  });
}
