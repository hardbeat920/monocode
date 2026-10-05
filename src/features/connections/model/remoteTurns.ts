import { useEffect, useRef } from "react";
import type { Session } from "../../sessions/model/session";
import {
  loadRemoteSession,
  REMOTE_CHANGES,
  remoteSessionFor,
  useRemoteMachines,
  watchRemoteChanges,
  type RemoteChangesDetail,
} from "./connections";
import type { HostSession } from "./protocol";
import { remoteProjectFor } from "./remoteProjects";

/** An open tab that shows a host session. */
export type RemoteTurnTarget = {
  shellId: string;
  machineId: string;
  hostSessionId: string;
  busy: boolean;
};

/**
 * What a batch of host writes means for open tabs: a turn started in a tab
 * that still looks idle, or a turn finished in a tab that still looks busy.
 * After a host restart every busy tab is rechecked.
 */
export function remoteTurnUpdates(
  targets: readonly RemoteTurnTarget[],
  detail: RemoteChangesDetail,
): { started: string[]; finished: RemoteTurnTarget[] } {
  const started: string[] = [];
  const finished: RemoteTurnTarget[] = [];
  for (const target of targets) {
    if (target.machineId !== detail.machineId) continue;
    if (detail.reset) {
      if (target.busy) finished.push(target);
      continue;
    }
    // The host sends each session's latest write once per batch.
    const change = detail.sessions.find(
      (entry) => entry.id === target.hostSessionId,
    );
    if (!change || change.busy === undefined) continue;
    if (change.busy && !target.busy) started.push(target.shellId);
    else if (!change.busy && target.busy) finished.push(target);
  }
  return { started, finished };
}

/**
 * Keeps every open remote tab's turn state current, including tabs that are
 * not showing and so do not poll their host. A finished turn loads the final
 * snapshot once, which lets the app announce it as it does a local turn.
 */
export function useRemoteTurnUpdates(
  sessions: readonly Session[],
  handlers: {
    onBusy: (shellId: string) => void;
    onSnapshot: (shellId: string, snapshot: HostSession) => void;
    known: (shellId: string) => HostSession | undefined;
  },
) {
  const { machines } = useRemoteMachines();
  const targets: RemoteTurnTarget[] = [];
  for (const session of sessions) {
    const project = remoteProjectFor(session.cwd);
    if (!project) continue;
    const machine = machines.find(
      (entry) => entry.environmentId === project.environmentId,
    );
    const hostSessionId = machine ? remoteSessionFor(session.id) : undefined;
    if (!machine || !hostSessionId) continue;
    targets.push({
      shellId: session.id,
      machineId: machine.id,
      hostSessionId,
      busy: !!session.busy,
    });
  }
  const latest = useRef(targets);
  latest.current = targets;
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;
  const machineKey = [...new Set(targets.map((target) => target.machineId))]
    .sort()
    .join("\n");
  useEffect(() => {
    if (!machineKey) return;
    const stops = machineKey.split("\n").map(watchRemoteChanges);
    const changed = (event: Event) => {
      const { started, finished } = remoteTurnUpdates(
        latest.current,
        (event as CustomEvent<RemoteChangesDetail>).detail,
      );
      for (const shellId of started) handlersRef.current.onBusy(shellId);
      for (const target of finished)
        void loadRemoteSession(
          target.machineId,
          target.hostSessionId,
          handlersRef.current.known(target.shellId),
        )
          .then((snapshot) => handlersRef.current.onSnapshot(target.shellId, snapshot))
          .catch(() => {
            /* the tab reloads when it is shown */
          });
    };
    window.addEventListener(REMOTE_CHANGES, changed);
    return () => {
      window.removeEventListener(REMOTE_CHANGES, changed);
      for (const stop of stops) stop();
    };
  }, [machineKey]);
}
