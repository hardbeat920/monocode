import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import {
  applySessionSync,
  type HostCommand,
  type HostSession,
  type HostSessionSummary,
  type RemoteMachine,
  type SessionSync,
  type SessionSyncChunk,
  type SessionSyncResponse,
} from "./protocol";
import { remoteProjectFor } from "./remoteProjects";

const CHANGE = "monocode:remote-machines";
export const REMOTE_HISTORY_CHANGE = "monocode:remote-history";
export const REMOTE_HISTORY_UPDATED = "monocode:remote-history-updated";
export const refreshRemoteProjectSessions = () =>
  window.dispatchEvent(new Event(REMOTE_HISTORY_CHANGE));
let cachedMachines: RemoteMachine[] = [];
let machinesLoaded = false;
export const OPEN_CONNECTIONS_EVENT = "monocode:open-connections";
export const OPEN_REMOTE_PROJECT_EVENT = "monocode:open-remote-project";
export const refreshRemoteMachines = () =>
  window.dispatchEvent(new Event(CHANGE));
const TAB_KEY = "monocode.remote-tabs.v2";
const WORKTREE_KEY = "monocode.remote-pending-worktrees.v1";

export function remotePendingWorktree(shellId: string): string | undefined {
  try {
    const value = JSON.parse(localStorage.getItem(WORKTREE_KEY) ?? "{}")[
      shellId
    ];
    return typeof value === "string" ? value : undefined;
  } catch {
    return undefined;
  }
}

/** The host checkout currently used by a remote tab. */
export function remoteTabCwd(project: string, shellId?: string): string | undefined {
  if (!shellId) return undefined;
  const sessionId = remoteSessionFor(shellId);
  return (
    (sessionId ? cachedRemoteSessionSummary(project, sessionId)?.cwd : undefined) ??
    remotePendingWorktree(shellId)
  );
}

export function rememberRemotePendingWorktree(shellId: string, path?: string) {
  try {
    const all = JSON.parse(localStorage.getItem(WORKTREE_KEY) ?? "{}");
    if (path) all[shellId] = path;
    else delete all[shellId];
    localStorage.setItem(WORKTREE_KEY, JSON.stringify(all));
  } catch {
    /* selection is restored from the host once a session exists */
  }
}

/** The host session a tab in a remote project shows; none for a new session. */
export function remoteSessionFor(shellId: string): string | undefined {
  try {
    const value = JSON.parse(localStorage.getItem(TAB_KEY) ?? "{}")[shellId];
    return typeof value === "string" ? value : undefined;
  } catch {
    return undefined;
  }
}
export function rememberRemoteSession(shellId: string, sessionId?: string) {
  try {
    const all = JSON.parse(localStorage.getItem(TAB_KEY) ?? "{}");
    if (sessionId) all[shellId] = sessionId;
    else delete all[shellId];
    localStorage.setItem(TAB_KEY, JSON.stringify(all));
  } catch {
    /* tab selection is best effort */
  }
  window.dispatchEvent(new Event(REMOTE_HISTORY_CHANGE));
}

const pendingPrefix = (project: string, environment: string) =>
  `monocode.remote-command.v1:${JSON.stringify([project, environment])}:`;

export const pendingRemoteCommand = (
  project: string,
  environment: string,
  sessionId?: string | null,
): HostCommand | undefined => {
  const prefix = pendingPrefix(project, environment);
  for (let index = 0; index < localStorage.length; index++) {
    const key = localStorage.key(index);
    if (key?.startsWith(prefix)) {
      const value = localStorage.getItem(key);
      if (value) {
        const command = JSON.parse(value) as HostCommand;
        if (
          sessionId === undefined ||
          (sessionId === null
            ? command.type === "create"
            : command.type !== "create" && command.sessionId === sessionId)
        )
          return command;
      }
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

/** Reads one sync, assembling it from bounded pieces when the host chunks it. */
async function syncRemoteSession(
  machineId: string,
  sessionId: string,
  revision?: number,
): Promise<SessionSync> {
  const response = await remoteRequest<SessionSyncResponse>(
    machineId,
    "sessions.sync",
    { sessionId, revision },
  );
  if (response.kind !== "chunked") return response;
  const pieces: string[] = [];
  let offset = 0;
  while (offset < response.length) {
    const { data } = await remoteRequest<SessionSyncChunk>(
      machineId,
      "sessions.syncChunk",
      { sessionId, transfer: response.transfer, offset },
    );
    if (!data) throw new Error("Session transfer ended early");
    pieces.push(data);
    offset += data.length;
  }
  if (offset !== response.length)
    throw new Error("Session transfer has an unexpected length");
  return JSON.parse(pieces.join("")) as SessionSync;
}

/** Fetches only what changed since `known`; falls back to a full snapshot. */
export async function loadRemoteSession(
  machineId: string,
  sessionId: string,
  known?: HostSession,
): Promise<HostSession> {
  const sync = (revision?: number) =>
    syncRemoteSession(machineId, sessionId, revision);
  const update = await sync(known?.revision);
  try {
    return applySessionSync(known, update);
  } catch {
    return applySessionSync(undefined, await sync());
  }
}

/** The connected machine for an environment, from the last machine list read. */
export function knownRemoteMachine(
  environmentId: string,
): RemoteMachine | undefined {
  return cachedMachines.find((entry) => entry.environmentId === environmentId);
}

/** The connected machine for an environment, reading the list when needed. */
export async function remoteMachineFor(
  environmentId: string,
): Promise<RemoteMachine | undefined> {
  const known = knownRemoteMachine(environmentId);
  if (known || machinesLoaded) return known;
  const value = await invoke<RemoteMachine[]>("remote_machines");
  cachedMachines = Array.isArray(value) ? value : [];
  machinesLoaded = true;
  return knownRemoteMachine(environmentId);
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
  cachedMachines = [
    ...cachedMachines.filter((entry) => entry.id !== machine.id),
    machine,
  ];
  machinesLoaded = true;
  window.dispatchEvent(new Event(CHANGE));
  return machine;
}

export async function disconnectMachine(machineId: string): Promise<void> {
  await invoke("remote_disconnect", { machineId });
  cachedMachines = cachedMachines.filter((entry) => entry.id !== machineId);
  window.dispatchEvent(new Event(CHANGE));
}

export function useRemoteMachines(enabled = true): {
  machines: RemoteMachine[];
  loaded: boolean;
} {
  const [state, setState] = useState<{
    machines: RemoteMachine[];
    loaded: boolean;
  }>({ machines: cachedMachines, loaded: machinesLoaded });
  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    const refresh = () => {
      void invoke<RemoteMachine[]>("remote_machines")
        .then((value) => {
          if (!disposed) {
            cachedMachines = Array.isArray(value) ? value : [];
            machinesLoaded = true;
            setState({
              machines: cachedMachines,
              loaded: true,
            });
          }
        })
        .catch(() => {
          // A temporary connection failure should not blank every remote
          // panel while a fresh machine list is requested.
          if (!disposed) setState({ machines: cachedMachines, loaded: true });
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

const STATUS = "monocode:remote-machine-status";
const machineOnline = new Map<string, boolean>();
const statusWatchers = new Map<
  string,
  { count: number; timer?: ReturnType<typeof setTimeout> }
>();

/** Records whether a machine answered its latest request, for every view
 * that shows its connection state. */
export function reportRemoteMachineStatus(machineId: string, online: boolean) {
  if (machineOnline.get(machineId) === online) return;
  machineOnline.set(machineId, online);
  window.dispatchEvent(new Event(STATUS));
}

function watchMachineStatus(machineId: string): () => void {
  const existing = statusWatchers.get(machineId);
  if (existing) {
    existing.count++;
  } else {
    const watcher: { count: number; timer?: ReturnType<typeof setTimeout> } =
      { count: 1 };
    statusWatchers.set(machineId, watcher);
    let failures = 0;
    const poll = async () => {
      try {
        await remoteRequest(machineId, "environment.describe");
        failures = 0;
        reportRemoteMachineStatus(machineId, true);
      } catch {
        failures = Math.min(4, failures + 1);
        reportRemoteMachineStatus(machineId, false);
      }
      if (statusWatchers.get(machineId) === watcher)
        watcher.timer = setTimeout(
          () => void poll(),
          failures ? Math.min(30_000, 3_000 * 2 ** failures) : 15_000,
        );
    };
    void poll();
  }
  return () => {
    const watcher = statusWatchers.get(machineId);
    if (!watcher || --watcher.count > 0) return;
    clearTimeout(watcher.timer);
    statusWatchers.delete(machineId);
  };
}

/** Whether a machine is reachable; undefined until the first check returns. */
export function useRemoteMachineOnline(machineId?: string): boolean | undefined {
  const [online, setOnline] = useState(() =>
    machineId ? machineOnline.get(machineId) : undefined,
  );
  useEffect(() => {
    if (!machineId) {
      setOnline(undefined);
      return;
    }
    const update = () => setOnline(machineOnline.get(machineId));
    update();
    window.addEventListener(STATUS, update);
    const unwatch = watchMachineStatus(machineId);
    return () => {
      window.removeEventListener(STATUS, update);
      unwatch();
    };
  }, [machineId]);
  return online;
}

const historyKey = (project: string) => `monocode.remote-history.v2:${project}`;

function cachedSessions(project: string): HostSessionSummary[] {
  try {
    const value: unknown = JSON.parse(
      localStorage.getItem(historyKey(project)) ?? "[]",
    );
    return Array.isArray(value) ? (value as HostSessionSummary[]) : [];
  } catch {
    return [];
  }
}
export function cachedRemoteSessionSummary(project: string, sessionId: string) {
  return cachedSessions(project).find((session) => session.id === sessionId);
}

export type RemoteProjectSessions = {
  /** Undefined when this machine is not connected on this computer. */
  machine?: RemoteMachine;
  sessions: HostSessionSummary[];
  loaded: boolean;
};

/** Lists a remote project's host sessions, keeping the last list visible
 * while the machine is unreachable. */
export function useRemoteProjectSessions(
  project: string,
  enabled = true,
): RemoteProjectSessions {
  const remote = enabled ? remoteProjectFor(project) : undefined;
  const { machines } = useRemoteMachines(!!remote);
  const machine = remote
    ? machines.find((entry) => entry.environmentId === remote.environmentId)
    : undefined;
  const [sessions, setSessions] = useState<HostSessionSummary[]>(() =>
    remote ? cachedSessions(project) : [],
  );
  const [loaded, setLoaded] = useState(false);
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    if (!remote) return;
    const changed = () => setRefresh((value) => value + 1);
    window.addEventListener(REMOTE_HISTORY_CHANGE, changed);
    return () => window.removeEventListener(REMOTE_HISTORY_CHANGE, changed);
  }, [!!remote]);
  useEffect(() => {
    setSessions(remote ? cachedSessions(project) : []);
    setLoaded(false);
    if (!remote || !machine) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    let failures = 0;
    const poll = async () => {
      try {
        const next = await remoteRequest<HostSessionSummary[]>(
          machine.id,
          "sessions.list",
          { projectId: remote.projectId },
        );
        if (disposed) return;
        failures = 0;
        setSessions(next);
        setLoaded(true);
        try {
          localStorage.setItem(historyKey(project), JSON.stringify(next));
          window.dispatchEvent(new Event(REMOTE_HISTORY_UPDATED));
        } catch {
          /* the list is refetched next time */
        }
      } catch {
        // Keep the cached list and back off while SSH is unavailable.
        failures = Math.min(4, failures + 1);
      }
      if (!disposed)
        timer = setTimeout(
          () => void poll(),
          failures ? Math.min(30_000, 3_000 * 2 ** failures) : 3_000,
        );
    };
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [project, remote?.projectId, machine?.id, refresh]);
  return { machine, sessions, loaded };
}
