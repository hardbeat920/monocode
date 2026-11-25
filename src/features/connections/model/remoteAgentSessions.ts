import type { ControlOutcome } from "../../orchestration/model/orchestration";
import { isRemoteProjectPath } from "../../projects/model/recents";
import {
  newSession,
  sessionDraftBlock,
  type Session,
} from "../../sessions/model/session";
import {
  cachedRemoteSessionSummary,
  loadRemoteSession,
  refreshRemoteProjectSessions,
  remoteMachineFor,
  remoteRequest,
  remoteSessionFor,
  updateCachedRemoteProjectSessions,
} from "./connections";
import type {
  CommandReceipt,
  HostCommand,
  HostSession,
  RemoteMachine,
} from "./protocol";
import {
  allRemoteProjects,
  remoteProjectFor,
  type RemoteProject,
} from "./remoteProjects";
import { remoteSessionState } from "./remoteSessionState";

export type RemoteAgentSessionTarget = {
  project: RemoteProject;
  hostId: string;
  openShell?: Session;
};

export type RemoteSettlementCallback = ((
  outcome: ControlOutcome,
  session?: Session,
) => void) & {
  accept: () => void;
  discard: () => void;
};

export type RemoteAgentSessionDeps = {
  remoteMachineFor: (environmentId: string) => Promise<RemoteMachine | undefined>;
  loadRemoteSession: (
    machineId: string,
    sessionId: string,
    known?: HostSession,
  ) => Promise<HostSession>;
  remoteRequest: <T>(
    machineId: string,
    method: string,
    params?: unknown,
  ) => Promise<T>;
  updateCachedRemoteProjectSessions: typeof updateCachedRemoteProjectSessions;
  refreshRemoteProjectSessions: typeof refreshRemoteProjectSessions;
  onRemoteSessionDeleted: (remoteSessionId: string) => void;
  randomId: () => string;
  pollIntervalMs: number;
};

const defaultDeps: RemoteAgentSessionDeps = {
  remoteMachineFor,
  loadRemoteSession,
  remoteRequest,
  updateCachedRemoteProjectSessions,
  refreshRemoteProjectSessions,
  onRemoteSessionDeleted: () => {},
  randomId: () => crypto.randomUUID(),
  pollIntervalMs: 1_000,
};

const activeWatchers = new Map<string, Set<() => void>>();

function registerWatcher(hostId: string, cancel: () => void): () => void {
  let set = activeWatchers.get(hostId);
  if (!set) {
    set = new Set();
    activeWatchers.set(hostId, set);
  }
  set.add(cancel);
  return () => {
    const current = activeWatchers.get(hostId);
    if (!current) return;
    current.delete(cancel);
    if (current.size === 0) activeWatchers.delete(hostId);
  };
}

export function cancelRemoteSessionWatchers(hostId: string): void {
  const set = activeWatchers.get(hostId);
  if (!set) return;
  for (const cancel of [...set]) cancel();
  activeWatchers.delete(hostId);
}

export function isRemoteSessionNotFoundError(error: unknown): boolean {
  return String(error).includes("Session not found on this machine");
}

function isDifferentPayloadReceiptError(error: unknown): boolean {
  return String(error).includes(
    "Command ID was already used with a different payload",
  );
}

/**
 * Resolves a session ID for `AgentAppHost` to a remote host session target
 * when it belongs to a remote project (either via an open mapped shell or as
 * a remote-only host session). Returns `undefined` for local sessions and
 * unsent blank remote tabs that do not have a host session yet.
 */
export function resolveRemoteAgentSessionTarget(
  id: string,
  options: {
    cwd?: string;
    openShell?: Session;
    hintedProjectCwd?: string;
    remoteSessionFor?: (shellId: string) => string | undefined;
    remoteProjectFor?: (path: string) => RemoteProject | undefined;
    allRemoteProjects?: () => RemoteProject[];
    cachedRemoteSessionSummary?: typeof cachedRemoteSessionSummary;
  } = {},
): RemoteAgentSessionTarget | undefined {
  const lookupSession = options.remoteSessionFor ?? remoteSessionFor;
  const lookupProject = options.remoteProjectFor ?? remoteProjectFor;
  const listProjects = options.allRemoteProjects ?? allRemoteProjects;
  const lookupSummary =
    options.cachedRemoteSessionSummary ?? cachedRemoteSessionSummary;

  if (options.openShell) {
    if (!isRemoteProjectPath(options.openShell.cwd)) return undefined;
    const hostId = lookupSession(options.openShell.id);
    if (!hostId) return undefined;
    const project = lookupProject(options.openShell.cwd);
    return project
      ? { project, hostId, openShell: options.openShell }
      : undefined;
  }

  const candidateCwd = options.cwd ?? options.hintedProjectCwd;
  if (candidateCwd) {
    if (!isRemoteProjectPath(candidateCwd)) return undefined;
    const project = lookupProject(candidateCwd);
    return project ? { project, hostId: id } : undefined;
  }

  const project = listProjects().find((entry) =>
    Boolean(lookupSummary(entry.key, id)),
  );
  return project ? { project, hostId: id } : undefined;
}

export function remoteHostSessionToSession(
  snapshot: HostSession,
  project: RemoteProject,
  openShell?: Session,
): Session {
  const host = snapshot.session;
  const base =
    openShell ??
    newSession(
      host.harness,
      project.key,
      host.model,
      host.runtimeMode,
      host.modelSettings,
    );
  return {
    ...remoteSessionState(
      { ...base, cwd: project.key },
      snapshot,
      project,
    ),
    id: host.id,
    cwd: project.key,
    busy: snapshot.status === "running" || !!host.busy,
  };
}

export function remoteCompletionOutcome(
  snapshot: HostSession,
  requestId: string,
): ControlOutcome {
  const blocks = snapshot.session.blocks;
  const start = blocks.findIndex(
    (block) =>
      block.role === "user" &&
      (block.appRequestId === requestId || block.id === requestId),
  );
  const turnBlocks = start >= 0 ? blocks.slice(start + 1) : [];
  let reply = "";
  let systemMessage = "";
  for (const block of turnBlocks) {
    if (block.role === "user") break;
    if (
      block.role === "assistant" &&
      !block.tool &&
      !block.internal &&
      block.text.trim()
    ) {
      reply = block.text;
    } else if (block.role === "system" && block.text.trim()) {
      systemMessage = block.text.trim();
    }
  }
  if (
    snapshot.status === "interrupted" ||
    systemMessage === "Stopped by you."
  ) {
    return {
      status: "cancelled",
      text: reply,
      ...(systemMessage ? { error: systemMessage } : {}),
    };
  }
  if (systemMessage) {
    return {
      status: "failed",
      text: reply,
      error: systemMessage,
    };
  }
  return { status: "completed", text: reply };
}

export async function loadRemoteAgentSession(
  target: RemoteAgentSessionTarget,
  deps: RemoteAgentSessionDeps = defaultDeps,
  options: {
    onSnapshot?: (shellId: string, snapshot: HostSession) => void;
  } = {},
): Promise<Session | null> {
  const machine = await deps.remoteMachineFor(target.project.environmentId);
  if (!machine) {
    throw new Error(
      "Connect this project's remote machine to read this session",
    );
  }
  try {
    const snapshot = await deps.loadRemoteSession(machine.id, target.hostId);
    if (target.openShell) {
      options.onSnapshot?.(target.openShell.id, snapshot);
    }
    return remoteHostSessionToSession(
      snapshot,
      target.project,
      target.openShell,
    );
  } catch (error) {
    if (isRemoteSessionNotFoundError(error)) return null;
    throw error;
  }
}

export function watchRemoteSessionCompletion(
  machineId: string,
  target: RemoteAgentSessionTarget,
  requestId: string,
  initialSnapshot: HostSession,
  onSettled: RemoteSettlementCallback,
  deps: RemoteAgentSessionDeps = defaultDeps,
  options: {
    onSnapshot?: (shellId: string, snapshot: HostSession) => void;
  } = {},
): () => void {
  let cancelled = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let latest: HostSession = initialSnapshot;
  let failures = 0;

  const cleanup = () => {
    cancelled = true;
    if (timer !== undefined) clearTimeout(timer);
    unregister();
  };
  const unregister = registerWatcher(target.hostId, cleanup);

  const poll = async () => {
    if (cancelled) return;
    try {
      latest = await deps.loadRemoteSession(machineId, target.hostId, latest);
      if (cancelled) return;
      failures = 0;
      if (target.openShell) {
        options.onSnapshot?.(target.openShell.id, latest);
      }
      if (latest.status !== "running") {
        cleanup();
        onSettled(
          remoteCompletionOutcome(latest, requestId),
          remoteHostSessionToSession(latest, target.project, target.openShell),
        );
        return;
      }
    } catch (error) {
      if (cancelled) return;
      if (isRemoteSessionNotFoundError(error)) {
        cleanup();
        onSettled({
          status: "failed",
          text: "",
          error: "Remote session is no longer available",
        });
        return;
      }
      failures += 1;
      if (failures >= 20) {
        cleanup();
        onSettled({
          status: "failed",
          text: "",
          error: error instanceof Error ? error.message : String(error),
        });
        return;
      }
    }
    if (!cancelled) {
      const delay = failures
        ? Math.min(15_000, deps.pollIntervalMs * 2 ** Math.min(4, failures))
        : deps.pollIntervalMs;
      timer = setTimeout(() => void poll(), delay);
    }
  };

  timer = setTimeout(() => void poll(), deps.pollIntervalMs);
  return cleanup;
}

export async function sendRemoteAgentSession(
  target: RemoteAgentSessionTarget,
  prompt: string,
  requestId: string,
  options: {
    rememberFollowUp?: (session: Session) => void;
    createSettler?: (session: Session) => RemoteSettlementCallback | undefined;
    onSnapshot?: (shellId: string, snapshot: HostSession) => void;
  } = {},
  deps: RemoteAgentSessionDeps = defaultDeps,
): Promise<{ alreadySubmitted: boolean }> {
  const machine = await deps.remoteMachineFor(target.project.environmentId);
  if (!machine) {
    throw new Error(
      "Connect this project's remote machine to send to this session",
    );
  }
  const snapshot = await deps.loadRemoteSession(machine.id, target.hostId);
  if (target.openShell) {
    options.onSnapshot?.(target.openShell.id, snapshot);
  }
  const session = remoteHostSessionToSession(
    snapshot,
    target.project,
    target.openShell,
  );
  const previous = snapshot.session.blocks.find(
    (block) => block.appRequestId === requestId || block.id === requestId,
  );
  if (previous) {
    if (previous.text !== prompt) {
      throw new Error("Request ID was already used with another prompt");
    }
    if (previous.draft) {
      throw new Error("Request ID belongs to an unsent draft");
    }
    options.rememberFollowUp?.(session);
    return { alreadySubmitted: true };
  }
  if (snapshot.status === "running" || snapshot.session.busy) {
    throw new Error("Session is busy; try again when it finishes");
  }
  if (sessionDraftBlock(snapshot.session)) {
    throw new Error("Session already has a draft; send or remove it first");
  }

  const onSettled = options.createSettler?.(session);
  const command: Extract<HostCommand, { type: "send" }> = {
    type: "send",
    commandId: requestId,
    sessionId: target.hostId,
    text: prompt,
    attachments: [],
    intent: "default",
  };
  try {
    await deps.remoteRequest<CommandReceipt>(
      machine.id,
      "commands.dispatch",
      command,
    );
  } catch (error) {
    onSettled?.discard();
    if (isDifferentPayloadReceiptError(error)) {
      throw new Error("Request ID was already used with another prompt");
    }
    throw error;
  }

  deps.updateCachedRemoteProjectSessions(target.project.key, (sessions) =>
    sessions.map((entry) =>
      entry.id === target.hostId
        ? { ...entry, status: "running", draft: false }
        : entry,
    ),
  );
  deps.refreshRemoteProjectSessions();
  options.rememberFollowUp?.(session);
  if (onSettled) {
    onSettled.accept();
    watchRemoteSessionCompletion(
      machine.id,
      target,
      requestId,
      snapshot,
      onSettled,
      deps,
      { onSnapshot: options.onSnapshot },
    );
  }
  return { alreadySubmitted: false };
}

export async function draftRemoteAgentSession(
  target: RemoteAgentSessionTarget,
  prompt: string,
  requestId: string,
  options: {
    onSnapshot?: (shellId: string, snapshot: HostSession) => void;
  } = {},
  deps: RemoteAgentSessionDeps = defaultDeps,
): Promise<{ alreadySaved: boolean; draft: boolean }> {
  const machine = await deps.remoteMachineFor(target.project.environmentId);
  if (!machine) {
    throw new Error(
      "Connect this project's remote machine to save a draft to this session",
    );
  }
  const snapshot = await deps.loadRemoteSession(machine.id, target.hostId);
  if (target.openShell) {
    options.onSnapshot?.(target.openShell.id, snapshot);
  }
  const previous = snapshot.session.blocks.find(
    (block) => block.appRequestId === requestId || block.id === requestId,
  );
  if (previous) {
    if (previous.text !== prompt) {
      throw new Error("Request ID was already used with another prompt");
    }
    return { alreadySaved: true, draft: !!previous.draft };
  }
  if (snapshot.status === "running" || snapshot.session.busy) {
    throw new Error("Session is busy; try again when it finishes");
  }
  if (sessionDraftBlock(snapshot.session)) {
    throw new Error("Session already has a draft; send or remove it first");
  }

  const command: Extract<HostCommand, { type: "draft" }> = {
    type: "draft",
    commandId: requestId,
    sessionId: target.hostId,
    text: prompt,
    attachments: [],
  };
  try {
    await deps.remoteRequest<CommandReceipt>(
      machine.id,
      "commands.dispatch",
      command,
    );
  } catch (error) {
    if (isDifferentPayloadReceiptError(error)) {
      throw new Error("Request ID was already used with another prompt");
    }
    throw error;
  }

  deps.updateCachedRemoteProjectSessions(target.project.key, (sessions) =>
    sessions.map((entry) =>
      entry.id === target.hostId ? { ...entry, draft: true } : entry,
    ),
  );
  deps.refreshRemoteProjectSessions();
  return { alreadySaved: false, draft: true };
}

export async function stopRemoteAgentSession(
  target: RemoteAgentSessionTarget,
  deps: RemoteAgentSessionDeps = defaultDeps,
): Promise<void> {
  const machine = await deps.remoteMachineFor(target.project.environmentId);
  if (!machine) {
    throw new Error(
      "Connect this project's remote machine to stop this session",
    );
  }
  const snapshot = await deps.loadRemoteSession(machine.id, target.hostId);
  if (snapshot.status === "running" && snapshot.runId) {
    const command: Extract<HostCommand, { type: "cancel" }> = {
      type: "cancel",
      commandId: deps.randomId(),
      sessionId: target.hostId,
      runId: snapshot.runId,
    };
    try {
      await deps.remoteRequest<CommandReceipt>(
        machine.id,
        "commands.dispatch",
        command,
      );
    } catch (error) {
      if (!String(error).includes("finished or replaced turn")) throw error;
    }
  }
  cancelRemoteSessionWatchers(target.hostId);
}

export async function removeRemoteAgentSession(
  target: RemoteAgentSessionTarget,
  mode: "archive" | "delete",
  deps: RemoteAgentSessionDeps = defaultDeps,
): Promise<void> {
  const machine = await deps.remoteMachineFor(target.project.environmentId);
  if (!machine) {
    throw new Error(
      `Connect this project's remote machine to ${mode === "archive" ? "archive" : "delete"} this session`,
    );
  }
  if (mode === "archive") {
    await deps.remoteRequest(machine.id, "sessions.update", {
      projectId: target.project.projectId,
      sessionId: target.hostId,
      archived: true,
    });
    deps.updateCachedRemoteProjectSessions(target.project.key, (sessions) =>
      sessions.map((entry) =>
        entry.id === target.hostId ? { ...entry, archived: true } : entry,
      ),
    );
  } else {
    await deps.remoteRequest(machine.id, "sessions.delete", {
      projectId: target.project.projectId,
      sessionId: target.hostId,
    });
    deps.updateCachedRemoteProjectSessions(target.project.key, (sessions) =>
      sessions.filter((entry) => entry.id !== target.hostId),
    );
  }
  deps.refreshRemoteProjectSessions();
  deps.onRemoteSessionDeleted(target.hostId);
  cancelRemoteSessionWatchers(target.hostId);
}
