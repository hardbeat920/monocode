import { isEditTool } from "../../../integrations/harness/core/preview";
import { displayPath } from "../../../shared/lib/paths";
import type { BuiltinSkill } from "../../skills/model/skills";
import type { Block } from "./session";

const FILE_LIMIT = 200;

export const STAGE_COMMAND: BuiltinSkill = {
  kind: "builtin",
  name: "stage",
  invocation: "stage",
  description: "Stage only the files this session changed.",
  scope: "builtin",
  source: "monocode",
};

/** Match the standalone composer command without consuming ordinary prompt text. */
export function isStageCommand(text: string): boolean {
  return /^\s*\/stage\s*$/i.test(text);
}

/** Consume `/stage` when it is the leading command, keeping any extra instructions. */
export function consumeStageCommand(text: string): {
  text: string;
  matched: boolean;
} {
  const match = text.match(/^\s*\/stage(?=\s|$)\s*/i);
  if (!match) return { text, matched: false };
  return { text: text.slice(match[0].length).trim(), matched: true };
}

/** Files the agent successfully edited anywhere in this session, relative to cwd. */
export function sessionChangedFiles(
  blocks: readonly Block[],
  cwd?: string,
): string[] {
  const files = new Set<string>();
  for (const block of blocks) {
    const tool = block.role === "tool" ? block.tool : undefined;
    if (
      !tool ||
      !["completed", "success"].includes(tool.status ?? "") ||
      !isEditTool(tool.kind, tool.title, tool.preview)
    )
      continue;
    for (const path of [...(tool.paths ?? []), tool.preview?.path]) {
      const label = path ? displayPath(path, cwd).trim() : "";
      if (label) files.add(label);
    }
  }
  return [...files].slice(0, FILE_LIMIT);
}

export function stageTurnPrompt(request: string, files: string[]): string {
  return [
    "Stage the changes you made in this session with git, and nothing else.",
    "",
    "- Stage only the files you created, edited, renamed, or deleted while working in this conversation. Name each path explicitly; do not use `git add .`, `git add -A`, `git add -u`, `git commit -a`, or any other repository-wide staging.",
    "- Leave every other change unstaged, including edits made by the user, other agents, or tools outside this conversation. Do not unstage anything that is already staged.",
    "- If a file you changed also contains changes you did not make, stage only your hunks when you can separate them reliably, for example by applying a patch of just those hunks with `git apply --cached`. Otherwise leave that file unstaged and say why.",
    "- Do not commit, push, stash, reset, check out, or otherwise modify the working tree.",
    "- Finish with the output of `git diff --cached --stat` and a short list of any changes you left unstaged.",
    ...(files.length
      ? [
          "",
          "Files MonoCode recorded you editing in this session (this list can be incomplete, so also rely on your own record of the conversation):",
          "",
          ...files.map((file) => `- ${file}`),
        ]
      : []),
    ...(request.trim()
      ? ["", "Additional instructions from the user:", "", request.trim()]
      : []),
  ].join("\n");
}
