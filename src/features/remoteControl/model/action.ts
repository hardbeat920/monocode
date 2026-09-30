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
  /**
   * Whether the mode will take this conversation over on its own.
   *
   * A boolean rather than the mode itself, so this module stays ignorant of the
   * setting. It changes what an *unbound* thread has to be told: under `manual`
   * nothing happens until the user acts, and under `all` it happens by itself and
   * there is nothing for them to do.
   */
  automatic?: boolean;
  /**
   * Whether a hand-over has been asked for and is waiting for the turn to end.
   * See `handoverTiming` — the request is held rather than refused.
   */
  queued?: boolean;
  /**
   * Whether a turn is running. The click is still offered — it is held rather
   * than refused — but it will not act immediately, and the label is the only
   * thing read before committing to it.
   */
  busy?: boolean;
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

  // Closing only ever applies to something already running, which by definition
  // had an id to start from, so neither the missing-id case nor the waiting ones
  // below can block it.
  if (target.active) {
    return {
      id: "remote-control",
      label: "Close Remote Control",
      intent,
      disabled: false,
    };
  }

  const label = "Open Remote Control";
  const waiting = (description: string): RemoteControlAction => ({
    id: "remote-control",
    label,
    intent,
    description,
    disabled: true,
  });

  // Already asked for. Saying so is what stops the menu inviting a second click
  // for an instruction already given, and the turn's safety is the other half of
  // the answer — the hand-over waits precisely so the turn is not thrown away.
  if (target.queued) {
    return waiting("Opens when this turn ends — the turn is not interrupted");
  }

  if (!target.providerSessionId) {
    // Same state, two different things to say, and telling the user to send a
    // message when the app will do it for them is the one that reads as a fault:
    // `all` mode hands a thread over the moment its first turn binds a
    // conversation, so until then nothing is wrong and nothing is required.
    return target.automatic
      ? waiting("Opens by itself once the first turn ends")
      : waiting(
          "Send a message first — there is no conversation to hand over yet",
        );
  }

  // Offered, and honest about when it lands. A turn running means the hand-over
  // is held until it ends — measured the hard way, by a click mid-turn throwing a
  // turn away before `handoverTiming` existed. The status line that reports the
  // hold arrives one moment too late to inform the click, and the label is what
  // is read before it. Only ever while a turn is genuinely running: hedging about
  // a turn that is not there would be its own small lie, and the click really is
  // immediate then.
  if (target.busy) {
    return {
      id: "remote-control",
      label: "Open Remote Control after this turn",
      intent,
      disabled: false,
    };
  }

  return { id: "remote-control", label, intent, disabled: false };
}
