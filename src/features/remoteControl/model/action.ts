import type { HarnessId } from "../../sessions/model/session";

/** What the user is asking for, decided by the menu rather than re-derived. */
export type RemoteControlIntent = "open" | "close";

export type RemoteControlTarget = {
  harness: HarnessId;
  /**
   * The id passed to `--resume`. Absent until the CLI binds one, which happens
   * on the first turn, so a fresh thread has nothing to hand over yet.
   */
  providerSessionId?: string;
  /**
   * Whether remote control is already running for this conversation. Comes from
   * the transcript's `bridge_status` record, so it is an input here rather than
   * something this module discovers.
   */
  active: boolean;
};

export type RemoteControlAction = {
  id: "remote-control";
  label: string;
  intent: RemoteControlIntent;
  /** Set only when the action is offered but cannot be used yet. */
  description?: string;
  disabled: boolean;
};

/**
 * The menu entry for one conversation, or `null` when it must not appear.
 *
 * Absent and disabled mean different things. A non-Claude harness has no remote
 * control at all, so the entry is absent the way `/resume` is. A Claude thread
 * that has not bound a session id yet is only *not yet* eligible, so it stays
 * visible and explains itself instead of silently disappearing.
 */
export function remoteControlAction(
  target: RemoteControlTarget,
): RemoteControlAction | null {
  if (target.harness !== "claude") return null;

  const intent: RemoteControlIntent = target.active ? "close" : "open";
  const label = target.active ? "Close Remote Control" : "Open Remote Control";

  // Closing only ever applies to something already running, which by definition
  // had an id to start from, so the missing-id case cannot block it.
  if (!target.active && !target.providerSessionId) {
    return {
      id: "remote-control",
      label,
      intent,
      description: "Send a message first — there is no conversation to hand over yet",
      disabled: true,
    };
  }

  return { id: "remote-control", label, intent, disabled: false };
}
