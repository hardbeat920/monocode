/**
 * The decisions behind wiring Remote Control into the workspace, kept out of
 * `App.tsx` so they can be tested without rendering it.
 *
 * Nothing here spawns, kills or reads anything — the effects live at the call
 * site in `App.tsx`, and these are the questions it has to answer first: what to
 * call a session on a phone, how an inbound message is seated, and when `all`
 * mode may take a conversation over.
 */

import type {
  RemoteControlIntent,
  RemoteControlTarget,
} from "../features/remoteControl/model/action";
import type { RemoteUserMessage } from "../features/remoteControl/model/transcript";
import type { RemoteControlMode } from "../features/settings/model/settings";
import type { Session } from "../features/sessions/model/session";
import { projectName } from "../shared/lib/paths";

export function remoteControlTarget(
  session: Session,
  active: boolean,
): RemoteControlTarget {
  return {
    harness: session.harness,
    providerSessionId: session.providerSessionId,
    active,
  };
}

/**
 * What to actually do about a click, given the state now.
 *
 * The menu decided the intent from what it could see when it opened. If that
 * disagrees with the state now — an `all`-mode open landed in between, or the
 * thread was archived — the click was aimed at something that has since changed,
 * so nothing happens. Flipping to the other action instead would carry out an
 * instruction the user never gave.
 */
export function remoteControlStep(
  intent: RemoteControlIntent,
  open: boolean,
): "open" | "close" | "none" {
  if (intent === "open") return open ? "none" : "open";
  return open ? "close" : "none";
}

/**
 * What the session is called on a phone.
 *
 * Assigned once, when the process starts, and never revisited: the CLI cannot be
 * renamed after launch, so a name derived from how many threads exist would
 * describe the wrong thread by the time someone read it. Hence the counter is
 * resolved against the names already in use rather than against a count.
 */
export function remoteControlName(
  cwd: string,
  taken: Iterable<string>,
): string {
  const base = projectName(cwd).trim() || "session";
  const used = new Set(taken);
  if (!used.has(base)) return base;
  let n = 2;
  while (used.has(`${base} ${n}`)) n += 1;
  return `${base} ${n}`;
}

/**
 * Seat a message that arrived from the phone or the TUI.
 *
 * It is seated as an ordinary user block because that is what it is: a user turn
 * on this conversation that did not come from this composer. `interjection` —
 * the only existing event carrying free text inward — appends a *system* block,
 * which would read as MonoCode remarking on the conversation rather than as part
 * of it. So the shape here matches what the composer itself appends.
 */
export function seatRemoteUserMessage(
  session: Session,
  event: RemoteUserMessage,
  now = Date.now(),
): Session {
  const text = event.text.trim();
  if (!text) return session;
  return {
    ...session,
    blocks: [
      ...session.blocks,
      { id: crypto.randomUUID(), role: "user", text, startedAt: now },
    ],
  };
}

/**
 * Tell the user their turn stopped, when opening Remote Control interrupted one.
 *
 * The hand-over stops the headless child, which ends whatever it was doing. A
 * turn still inside startup is superseded by the stop epoch and returns without
 * an error of its own, so nothing else would say anything and the user would
 * watch their message sit there unanswered. A turn already streaming fails its
 * next write and reports that, in which case this block explains a failure that
 * would otherwise look unprovoked.
 */
export function noteInterruptedTurn(session: Session): Session {
  if (!session.busy) return session;
  return {
    ...session,
    blocks: [
      ...session.blocks,
      {
        id: crypto.randomUUID(),
        role: "system",
        text: "Opening Remote Control stopped the turn that was running. Send it again from the phone, or close Remote Control and send it here.",
        notice: "error",
      },
    ],
  };
}

export type AutoOpenState = {
  mode: RemoteControlMode;
  /** Already handed over. */
  open: boolean;
  /** Closed by hand. `all` must not argue with the user about it. */
  dismissed: boolean;
};

/**
 * Whether `all` mode may take this session over now.
 *
 * "Lazily" is pinned to the session going idle with a conversation bound, which
 * is not a preference but the only moment that satisfies the hand-over's own
 * constraints. Before a first turn there is no `providerSessionId`, so there is
 * nothing for `--resume` to open. During a turn the hand-over would stop the
 * child the user is waiting on. And because a thread nobody has used never
 * reaches either state, processes accumulate against *used* threads rather than
 * open tabs — which is the cost §9.6 asks to avoid.
 */
export function shouldAutoOpen(
  session: Session,
  state: AutoOpenState,
): boolean {
  if (state.mode !== "all") return false;
  if (state.open || state.dismissed) return false;
  if (session.harness !== "claude") return false;
  if (!session.providerSessionId) return false;
  return !session.busy;
}
