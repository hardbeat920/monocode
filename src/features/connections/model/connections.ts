import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import {
  applySessionSync,
  type HostCommand,
  type HostProject,
  type HostSession,
  type RemoteMachine,
  type SessionSync,
} from "./protocol";

const CHANGE = "monocode:remote-machines";
export const OPEN_CONNECTIONS_EVENT = "monocode:open-connections";
export const refreshRemoteMachines = () => window.dispatchEvent(new Event(CHANGE));
const KEY = "monocode.remote-projects.v1";
type ProjectConnections = {
  machineId?: string;
  workspaces?: Record<string, HostProject>;
  sessions?: Record<string, string>;
  drafts?: Record<string, string>;
};

function read(project: string): ProjectConnections {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}")[project] ?? {};
  } catch {
    return {};
  }
}
function update(project: string, patch: Partial<ProjectConnections>) {
  try {
    const all = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    all[project] = { ...read(project), ...patch };
    localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    /* preferences are best effort; host state is durable */
  }
}

export const rememberedMachine = (project: string): string | undefined =>
  read(project).machineId;
export const rememberMachine = (project: string, machineId?: string) =>
  update(project, { machineId });
export const workspaceFor = (
  project: string,
  environment: string,
): HostProject | undefined => read(project).workspaces?.[environment];
export const rememberWorkspace = (
  project: string,
  environment: string,
  workspace: HostProject,
) =>
  update(project, {
    workspaces: { ...read(project).workspaces, [environment]: workspace },
  });
export const rememberedSession = (
  project: string,
  environment: string,
): string | undefined => read(project).sessions?.[environment];
export const rememberSession = (
  project: string,
  environment: string,
  sessionId: string,
) =>
  update(project, {
    sessions: { ...read(project).sessions, [environment]: sessionId },
  });
export const remoteDraft = (project: string, key: string): string =>
  read(project).drafts?.[key] ?? "";
export const saveRemoteDraft = (project: string, key: string, draft: string) =>
  update(project, { drafts: { ...read(project).drafts, [key]: draft } });
const pendingPrefix = (project: string, environment: string) =>
  `monocode.remote-command.v1:${JSON.stringify([project, environment])}:`;

export const pendingRemoteCommand = (
  project: string,
  environment: string,
): HostCommand | undefined => {
  const prefix = pendingPrefix(project, environment);
  for (let index = 0; index < localStorage.length; index++) {
    const key = localStorage.key(index);
    if (key?.startsWith(prefix)) {
      const value = localStorage.getItem(key);
      if (value) return JSON.parse(value) as HostCommand;
    }
  }
};

// Each command owns its storage entry: a late receipt from another pane can
// never erase this pane's uncertain request. Persistence must succeed before
// dispatch; unlike preferences, silently dropping an outbox entry is unsafe.
export const savePendingRemoteCommand = (
  project: string,
  environment: string,
  command: HostCommand,
) => {
  try {
    localStorage.setItem(
      `${pendingPrefix(project, environment)}${command.commandId}`,
      JSON.stringify(command),
    );
  } catch {
    throw new Error(
      "Cannot save your request locally. Free up app storage before sending.",
    );
  }
};
export const clearPendingRemoteCommand = (
  project: string,
  environment: string,
  commandId: string,
) =>
  localStorage.removeItem(`${pendingPrefix(project, environment)}${commandId}`);

export function remoteRequest<T>(
  machineId: string,
  method: string,
  params: unknown = {},
): Promise<T> {
  return invoke<T>("remote_request", { machineId, method, params });
}

/** Fetches only what changed since `known`; falls back to a full snapshot. */
export async function loadRemoteSession(
  machineId: string,
  sessionId: string,
  known?: HostSession,
): Promise<HostSession> {
  const sync = (revision?: number) =>
    remoteRequest<SessionSync>(machineId, "sessions.sync", {
      sessionId,
      revision,
    });
  const update = await sync(known?.revision);
  try {
    return applySessionSync(known, update);
  } catch {
    return applySessionSync(undefined, await sync());
  }
}

export async function connectMachine(
  name: string,
  url: string,
  token: string,
): Promise<RemoteMachine> {
  const machine = await invoke<RemoteMachine>("remote_connect", {
    name,
    url,
    token,
  });
  window.dispatchEvent(new Event(CHANGE));
  return machine;
}

export async function disconnectMachine(machineId: string): Promise<void> {
  await invoke("remote_disconnect", { machineId });
  window.dispatchEvent(new Event(CHANGE));
}

export function useRemoteMachines(enabled = true): {
  machines: RemoteMachine[];
  loaded: boolean;
} {
  const [state, setState] = useState<{
    machines: RemoteMachine[];
    loaded: boolean;
  }>({ machines: [], loaded: false });
  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    const refresh = () => {
      void invoke<RemoteMachine[]>("remote_machines")
        .then((value) => {
          if (!disposed)
            setState({
              machines: Array.isArray(value) ? value : [],
              loaded: true,
            });
        })
        .catch(() => {
          if (!disposed) setState({ machines: [], loaded: true });
        });
    };
    refresh();
    window.addEventListener(CHANGE, refresh);
    return () => {
      disposed = true;
      window.removeEventListener(CHANGE, refresh);
    };
  }, [enabled]);
  return state;
}
