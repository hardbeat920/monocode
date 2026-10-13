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

/** Status line after `/browser` alone switched the grant (spec 13.2). */
export function browserGrantStatus(enabled: boolean): string {
  return enabled
    ? "Browser access is on for this thread. The agent opens its own tabs in your browser; an agent that is already running may need its next turn or a restart to see the browser tools."
    : "Browser access is off for this thread. Its tabs stay open in your browser.";
}
