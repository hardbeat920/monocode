import { t } from "../../../shared/i18n/locale";
import type { BuiltinSkill } from "../../skills/model/skills";

export const COMPACT_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "compact",
  invocation: "compact",
  get description() {
    return t("sessions.command.compact.description");
  },
  scope: "builtin",
  source: "monocode",
};

/** Match the standalone composer command without consuming ordinary prompt text. */
export function isCompactCommand(text: string): boolean {
  return /^\s*\/compact\s*$/i.test(text);
}
