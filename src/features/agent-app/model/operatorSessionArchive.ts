import { shouldPersistSession, type SessionSummary } from "../../sessions/data/sessionStore";
import type { Session } from "../../sessions/model/session";

export type OperatorArchiveSession = Pick<SessionSummary, "archived">;

export type OperatorArchiveResult = { changed: boolean; archived: boolean };

export type OperatorArchiveAdapter = {
  callerId: string;
  /** Read the latest renderer copy after any awaited store lookup. */
  liveSession(id: string): Session | undefined;
  /** Include live UI activity and active orchestrator work. */
  isBusy(id: string): boolean;
  /** Find saved history within the requesting project's scope. */
  persistedSession(id: string): Promise<OperatorArchiveSession | undefined>;
  /** Close through UI safeguards and return storage failures unchanged. */
  archiveView(id: string): Promise<{
    completed: boolean;
    changed: boolean;
    error?: unknown;
  }>;
  /** Persist the archive flag and report whether its value changed. */
  setArchived(id: string, archived: boolean): Promise<boolean>;
};

/** Apply an Operator archive request through persisted state and the UI lifecycle. */
export async function archiveOperatorSession(
  id: string,
  archived: boolean,
  adapter: OperatorArchiveAdapter,
): Promise<OperatorArchiveResult> {
  if (archived && id === adapter.callerId)
    throw new Error("The current session cannot be archived by the app CLI");

  const persisted = await adapter.persistedSession(id);
  const live = adapter.liveSession(id);
  if (!persisted && !live)
    throw new Error("Session was not found in this project");
  if (archived && !persisted && (!live || !shouldPersistSession(live)))
    throw new Error("An empty session cannot be archived before it is saved");
  if (!archived && !persisted)
    throw new Error("Session has no saved history to unarchive");

  if (archived) {
    if (adapter.isBusy(id))
      throw new Error("Session is busy; try again when it finishes");
    const result = await adapter.archiveView(id);
    if ("error" in result) throw result.error;
    if (!result.completed)
      throw new Error("Archive was cancelled or the session became busy");
    return { archived: true, changed: result.changed };
  }

  return {
    archived: false,
    changed: await adapter.setArchived(id, false),
  };
}
