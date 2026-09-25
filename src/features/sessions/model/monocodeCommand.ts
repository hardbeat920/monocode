import type { BuiltinSkill } from "../../skills/model/skills";
import type { Block } from "./session";

export const MONOCODE_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "mono",
  invocation: "mono",
  description: "Enable MonoCode sessions, folders, and notes in this thread.",
  scope: "builtin",
  source: "monocode",
};

/** Activate app access with a leading composer command. */
export function consumeMonocodeCommand(text: string): {
  text: string;
  matched: boolean;
} {
  // Keep the old spelling as an unlisted alias for existing threads.
  const match = text.match(/^\s*\/(?:mono|monocode)(?=\s|$)\s*/i);
  if (!match) return { text, matched: false };
  return { text: text.slice(match[0].length), matched: true };
}

const LEGACY_COMMAND = /^\s*\/monocode(?=\s|$)\s*/i;

/** A submitted /mono turn keeps CLI access available in later turns. */
export function isMonocodeUserTurn(block: Block): boolean {
  return (
    block.role === "user" &&
    !block.draft &&
    !block.internal &&
    (block.monocode === true || LEGACY_COMMAND.test(block.text))
  );
}

/** Old /monocode messages remain enabled and render without the old prefix. */
export function monocodeUserPrompt(block: Block): string {
  const legacy = block.text.match(LEGACY_COMMAND);
  if (!legacy) return block.text;
  return (
    block.text.slice(legacy[0].length).trim() ||
    "Explain what you can do in MonoCode with the app CLI."
  );
}

export function monocodeEnabledInThread(blocks: Block[]): boolean {
  return blocks.some(isMonocodeUserTurn);
}
