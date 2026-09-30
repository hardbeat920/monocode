import type { BuiltinSkill } from "../../skills/model/skills";

/**
 * Opens the picker of conversations Claude Code already recorded for this
 * project, so one can be continued in MonoCode. Named `/resume` because that
 * is what people reach for after using the CLI, even though the CLI's own
 * `/resume` is an interactive screen that headless mode has no equivalent of.
 */
export const RESUME_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "resume",
  invocation: "resume",
  description: "Continue a conversation from Claude Code in this project.",
  scope: "builtin",
  source: "monocode",
};
