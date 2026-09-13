import { isStandaloneCommand, type BuiltinSkill } from "./skills";

export const COMPACT_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "compact",
  invocation: "compact",
  description: "Summarize older conversation context to free space.",
  scope: "builtin",
  source: "monocode",
};

export function isCompactCommand(text: string): boolean {
  return isStandaloneCommand(text, COMPACT_COMMAND.name);
}
