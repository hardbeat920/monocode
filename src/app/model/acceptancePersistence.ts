type Save = () => Promise<void>;
type Entry = {
  switchId: string;
  promise: Promise<void>;
  failed: boolean;
  onFailure: (error: AcceptancePersistenceError) => void;
  onSaved?: () => void;
};

export class AcceptancePersistenceError extends Error {
  readonly cause: unknown;

  constructor(cause: unknown) {
    super(
      "The provider accepted this request, but MonoCode could not save its acceptance. Further requests are paused. Try sending again to retry the save without sending provider input.",
    );
    this.cause = cause;
    this.name = "AcceptancePersistenceError";
  }
}

export function withAcceptancePersistenceError(
  session: Session,
  error: Error,
): Session {
  return {
    ...session,
    queueStatus: "paused",
    blocks: [
      ...session.blocks,
      {
        id: crypto.randomUUID(),
        role: "system",
        text: error.message,
        notice: "error",
      },
    ],
  };
}

export async function dispatchAfterContextSave(
  save: Save,
  isCurrent: () => boolean,
  dispatch: () => Promise<void>,
): Promise<void> {
  await save();
  if (isCurrent()) await dispatch();
}

export async function persistNativeContextDelivery(
  mode: "native" | "inline",
  save: Save,
): Promise<void> {
  if (mode === "native") await save();
}

export function createAcceptancePersistence() {
  const entries = new Map<string, Entry>();
  const start = (
    sessionId: string,
    switchId: string,
    save: Save,
    onFailure: Entry["onFailure"],
    onSaved?: Entry["onSaved"],
  ): Promise<void> => {
    const entry: Entry = {
      switchId,
      promise: Promise.resolve(),
      failed: false,
      onFailure,
      onSaved,
    };
    entries.set(sessionId, entry);
    entry.promise = Promise.resolve()
      .then(save)
      .then(() => {
        if (entries.get(sessionId) === entry) {
          entries.delete(sessionId);
          entry.onSaved?.();
        }
      })
      .catch((cause: unknown) => {
        const error = new AcceptancePersistenceError(cause);
        entry.failed = true;
        entry.onFailure(error);
        throw error;
      });
    // The provider callback cannot await storage. Completion and later submit
    // guards observe the same rejecting promise.
    void entry.promise.catch(() => undefined);
    return entry.promise;
  };
  return {
    start,
    hasPending: (sessionId: string) => entries.has(sessionId),
    hasFailed: (sessionId: string) => entries.get(sessionId)?.failed === true,
    submissionMode: (
      sessionId: string,
      busy: boolean,
    ): "submit" | "queue" | "wait" | "reconcile" => {
      const entry = entries.get(sessionId);
      if (!entry) return "submit";
      if (entry.failed) return "reconcile";
      return busy ? "queue" : "wait";
    },
    wait: (sessionId: string, switchId?: string) => {
      const entry = entries.get(sessionId);
      return entry && (!switchId || entry.switchId === switchId)
        ? entry.promise
        : Promise.resolve();
    },
    reconcile: (sessionId: string, save: Save): Promise<void> => {
      const entry = entries.get(sessionId);
      if (!entry) return Promise.resolve();
      return entry.failed
        ? start(sessionId, entry.switchId, save, entry.onFailure, entry.onSaved)
        : entry.promise;
    },
  };
}
import type { Session } from "../../features/sessions/model/session";
