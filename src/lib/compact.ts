import type { BuiltinSkill } from "./skills";

export const COMPACT_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "compact",
  invocation: "compact",
  description: "Summarize older conversation context to free space.",
  scope: "builtin",
  source: "monocode",
};

/** Match the standalone composer command without consuming ordinary prompt text. */
export function isCompactCommand(text: string): boolean {
  return /^\s*\/compact\s*$/i.test(text);
}
