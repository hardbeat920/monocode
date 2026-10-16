import type { BrowserAccessMode } from "../../settings/model/browser";
import type { BuiltinSkill } from "../../skills/model/skills";

export const BROWSER_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "browser",
  invocation: "browser",
  description: "Let this thread open and use its own tabs in your browser.",
  scope: "builtin",
  source: "monocode",
};

/**
 * `/browser` at the start of a prompt, or right after a leading `/operator`
 * (the Operator pill prefixes its own command). The rest of the prompt keeps
 * any operator prefix, so the operator parser still sees it.
 */
export function consumeBrowserCommand(text: string): {
  text: string;
  matched: boolean;
} {
  const match = text.match(
    /^\s*(\/(?:operator|mono|monocode)(?=\s|$)\s*)?\/browser(?=\s|$)\s*/i,
  );
  if (!match) return { text, matched: false };
  return {
    text: (match[1] ?? "") + text.slice(match[0].length),
    matched: true,
  };
}

/**
 * The grant `/browser` sets, or null when it is already right. Alone it
 * switches; with a prompt it only turns access on, so a thread that has
 * access already makes no grant call at all.
 */
export function browserGrantChange(
  granted: boolean,
  bare: boolean,
): boolean | null {
  if (bare) return !granted;
  return granted ? null : true;
}

/** Status line after `/browser` alone switched the grant. */
export function browserGrantStatus(enabled: boolean): string {
  return enabled
    ? "Browser access is on for this thread. The agent opens its own tabs; an agent that is already running may need its next turn or a restart to see the browser tools."
    : "Browser access is off for this thread. Its tabs stay open, and the agent asks you before it uses the browser again.";
}

/**
 * Whether a turn may get the browser on its own (Always on, Full access or
 * Ask) and use `/browser`: only a turn the user sends from a regular
 * thread. Turns an agent sends through the app (a session it starts, a
 * follow-up it hands over), orchestration and managed turns never do; they
 * use the browser only with a grant the thread already holds, such as one
 * a Mono passes to the sessions it starts.
 */
export function browserTurnEligible(turn: {
  managed?: boolean;
  /** An agent app request delivered this turn (`sessions.start`/`send`). */
  appRequestId?: string;
  /** A turn an agent launched through the app. */
  browserGrantOnly?: boolean;
  /** An orchestration lead, worker or run owns the turn. */
  orchestration: boolean;
}): boolean {
  return !(
    turn.managed ||
    turn.appRequestId ||
    turn.browserGrantOnly ||
    turn.orchestration
  );
}

export type BrowserTurnAccess = "granted" | "grant" | "ask" | null;

/**
 * What a turn without `/browser` does about the browser:
 * - `granted`: the thread has access; the turn carries the browser tools.
 * - `grant`: "Always on", or a thread the user gave "Full access", grants
 *   the thread first, then carries the tools.
 * - `ask`: the turn carries the tools without a grant; the first browser
 *   action asks the user in the thread and waits for the answer.
 * - null: no browser tools.
 *
 * `eligible` is `browserTurnEligible`: orchestration, managed and
 * agent-sent turns never get the browser on their own, though a grant the
 * thread already holds still counts. A thread the user turned off asks
 * instead of being granted again, with "Full access" too: turning it off is
 * the more specific choice.
 */
export function browserTurnAccess(input: {
  toggleOn: boolean;
  access: BrowserAccessMode;
  granted: boolean;
  eligible: boolean;
  turnedOff: boolean;
  /** The thread runs with "Full access" in the composer's access picker. */
  fullAccess?: boolean;
}): BrowserTurnAccess {
  if (!input.toggleOn) return null;
  if (input.granted) return "granted";
  if (!input.eligible) return null;
  const always = input.access === "always" || input.fullAccess === true;
  return always && !input.turnedOff ? "grant" : "ask";
}

/**
 * Whether a session a Mono started takes the Mono's browser grant when the
 * Mono starts it or hands it work. Only while the Mono holds a grant (which
 * needs the main switch on), and never for a session the user turned off.
 */
export function inheritsMonoBrowserGrant(input: {
  monoGranted: boolean;
  granted: boolean;
  turnedOff: boolean;
}): boolean {
  return input.monoGranted && !input.granted && !input.turnedOff;
}

/** The line an "ask" turn adds to the `<monocode_browser>` block. */
export const BROWSER_ASK_NOTE =
  "This thread has no browser access yet. Your first browser action asks the user in MonoCode and waits for the answer. If it returns not_granted, continue without the browser and do not try it again in this turn.";

/** Puts the "ask" note inside the browser block, or after it. */
export function withBrowserAskNote(block: string): string {
  const close = "</monocode_browser>";
  const end = block.lastIndexOf(close);
  if (end < 0) return `${block}\n${BROWSER_ASK_NOTE}`;
  return `${block.slice(0, end).trimEnd()}\n${BROWSER_ASK_NOTE}\n${block.slice(end)}`;
}
