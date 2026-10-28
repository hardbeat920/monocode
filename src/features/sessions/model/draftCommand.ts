import type { BuiltinSkill } from "../../skills/model/skills";

export const DRAFT_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "draft",
  invocation: "draft",
  description: "Save this message without starting the agent.",
  scope: "builtin",
  source: "monocode",
};

/** Consume `/draft` when it is used as the leading composer command. */
export function consumeDraftCommand(text: string): {
  text: string;
  matched: boolean;
} {
  const match = text.match(/^\s*\/draft(?=\s|$)\s*/i);
  if (!match) return { text, matched: false };
  return { text: text.slice(match[0].length), matched: true };
}
