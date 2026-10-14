import { t } from "../../../shared/i18n/locale";
import type { BuiltinSkill } from "../../skills/model/skills";

export const MCP_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "mcp",
  invocation: "mcp",
  get description() {
    return t("sessions.command.mcp.description");
  },
  scope: "builtin",
  source: "monocode",
};

export function isMcpCommand(text: string): boolean {
  return /^\s*\/mcp\s*$/i.test(text);
}
