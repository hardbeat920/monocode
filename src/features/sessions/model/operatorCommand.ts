import type { BuiltinSkill } from "../../skills/model/skills";
import type { Block } from "./session";

export const OPERATOR_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "operator",
  invocation: "operator",
  description:
    "Give this thread access to MonoCode sessions, folders, and notes.",
  scope: "builtin",
  source: "monocode",
};

/** Activate app access with a leading composer command. */
export function consumeOperatorCommand(text: string): {
  text: string;
  matched: boolean;
} {
  // Keep the old spellings as unlisted aliases for existing drafts and threads.
  const match = text.match(/^\s*\/(?:operator|mono|monocode)(?=\s|$)\s*/i);
  if (!match) return { text, matched: false };
  return { text: text.slice(match[0].length), matched: true };
}

const LEGACY_COMMAND = /^\s*\/(?:mono|monocode)(?=\s|$)\s*/i;

/** A submitted /operator turn keeps CLI access available in later turns. */
export function isOperatorUserTurn(block: Block): boolean {
  return (
    block.role === "user" &&
    !block.draft &&
    !block.internal &&
    // This persisted field keeps its original name for saved session compatibility.
    (block.monocode === true || LEGACY_COMMAND.test(block.text))
  );
}

/** Old command messages remain enabled and render without their old prefix. */
export function operatorUserPrompt(block: Block): string {
  const legacy = block.text.match(LEGACY_COMMAND);
  if (!legacy) return block.text;
  return (
    block.text.slice(legacy[0].length).trim() ||
    "Explain what you can do in MonoCode with the app CLI."
  );
}

export function operatorEnabledInThread(blocks: Block[]): boolean {
  return blocks.some(isOperatorUserTurn);
}
