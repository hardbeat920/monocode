import { useEffect, useSyncExternalStore } from "react";
import { Alert } from "react-native";

import { useConnection } from "@/features/connection/ConnectionProvider";
import {
  useDefaultModel,
  useDefaultRuntimeMode,
} from "@/features/settings/defaultModel";
import type { Harness, Project } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";

import { DEFAULT_ACCESS } from "./access";
import type { ModelPick, RuntimeMode } from "./types";

/** What the model picker set for a session, or for a new one in a project. */
type Picked = {
  model: { pick: ModelPick; name: string } | null;
  access: RuntimeMode | null;
  accounts: Partial<Record<Harness, string>>;
};

const NOTHING: Picked = { model: null, access: null, accounts: {} };

const choiceScope = (
  desktop: string | undefined,
  projectId: string | undefined,
  sessionId?: string,
) => `${desktop}\n${projectId}\n${sessionId ?? ""}`;

// Held here rather than by the screen, so the picker, a route of its own over
// it, sets what the screen shows. Keyed by project and session.
const picks = new Map<string, Picked>();
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
const set = (scope: string, next: Partial<Picked>) => {
  picks.set(scope, { ...(picks.get(scope) ?? NOTHING), ...next });
  listeners.forEach((listener) => listener());
};

// A slider drag picks step after step; the desktop hears only where it stops.
const applying = new Map<string, ReturnType<typeof setTimeout>>();

const failed = (reason: unknown) =>
  Alert.alert(
    "Could not change the session",
    reason instanceof Error ? reason.message : String(reason),
  );

/**
 * What a session, or a new one in the project, runs with: the picker's
 * choice, else the session's own, or for a new one this phone's defaults and
 * then the desktop's. Picks for a session go to the desktop as they're made.
 */
export function useSessionChoice(
  project: Project | undefined,
  sessionId: string | undefined,
) {
  const { sessions, configureSession } = useWorkspace();
  const desktop = useConnection().pairing?.desktop.fingerprint;
  const defaultModel = useDefaultModel();
  const defaultAccess = useDefaultRuntimeMode();
  const session = sessionId
    ? sessions.find((entry) => entry.id === sessionId)
    : undefined;
  const scope = choiceScope(desktop, project?.id, sessionId);
  const picked = useSyncExternalStore(
    subscribe,
    () => picks.get(scope) ?? NOTHING,
  );

  // A new session starts on the phone's default model, if Providers set one.
  const chosen = picked.model ?? (sessionId ? null : defaultModel);
  const access =
    picked.access ??
    (sessionId
      ? session?.runtimeMode
      : (defaultAccess ?? project?.defaultRuntimeMode)) ??
    DEFAULT_ACCESS;

  const pickModel = (pick: ModelPick, name: string) => {
    set(scope, { model: { pick, name } });
    if (!sessionId) return;
    clearTimeout(applying.get(scope));
    applying.set(
      scope,
      setTimeout(() => {
        applying.delete(scope);
        configureSession(sessionId, pick).catch(failed);
      }, 400),
    );
  };
  const pickAccess = (mode: RuntimeMode) => {
    set(scope, { access: mode });
    if (sessionId)
      configureSession(sessionId, { runtimeMode: mode }).catch(failed);
  };
  const pickAccount = (pick: ModelPick, name: string, accountId: string) => {
    // Account-owned provider threads stay with their original chat. Prepare a
    // new draft with the same model and permissions before opening it.
    const target = sessionId ? choiceScope(desktop, project?.id) : scope;
    set(target, {
      model: {
        pick: {
          harness: pick.harness,
          model: pick.model,
          modelSettings: pick.modelSettings,
        },
        name,
      },
      ...(sessionId ? { access } : {}),
      accounts: {
        ...(picks.get(target)?.accounts ?? {}),
        [pick.harness]: accountId,
      },
    });
  };

  return {
    session,
    chosen,
    access,
    accounts: picked.accounts,
    /** A new session's mode: picked or the phone's default, else the desktop's at send time. */
    startAccess: picked.access ?? defaultAccess,
    pickModel,
    pickAccess,
    pickAccount,
  };
}

/**
 * Forgets the picks for a session, or a new one in the project, once its
 * screen goes, so it opens again with nothing picked.
 */
export function useForgetChoiceOnLeave(
  projectId: string,
  sessionId: string | undefined,
) {
  const desktop = useConnection().pairing?.desktop.fingerprint;
  const scope = choiceScope(desktop, projectId, sessionId);
  useEffect(
    () => () => {
      picks.delete(scope);
    },
    [scope],
  );
}
