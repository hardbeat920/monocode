import type { BuiltinSkill } from "../../skills/model/skills";

export const MCP_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "mcp",
  invocation: "mcp",
  description: "Open MCP server settings and authentication.",
  scope: "builtin",
  source: "monocode",
};

export function isMcpCommand(text: string): boolean {
  return /^\s*\/mcp\s*$/i.test(text);
}
