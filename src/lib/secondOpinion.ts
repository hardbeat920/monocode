import { isEditTool } from "./harness/preview";
import { limitSection } from "./jsonText";
import { displayPath } from "./paths";
import {
  HARNESSES,
  HARNESS_TITLE,
  type Block,
  type HarnessId,
  type SecondOpinionMeta,
} from "./session";

const USER_LIMIT = 400;
const REPORT_LIMIT = 900;
const PROMPT_LIMIT = 1_800;

export const SECOND_OPINION_TITLE = "Second opinion";

/** Which provider produced this turn, walking back through handoff dividers. */
export function harnessForTurn(
  blocks: Block[],
  turn: Block[],
  sessionHarness: HarnessId,
): HarnessId {
  const startId = turn[0]?.id;
  const start = startId
    ? blocks.findIndex((block) => block.id === startId)
    : -1;
  if (start > 0) {
    for (let i = start - 1; i >= 0; i--) {
      const handoff = blocks[i]?.handoff;
      if (handoff) return handoff.to;
    }
  }
  const first = blocks.find((block) => block.handoff)?.handoff;
  return first?.from ?? sessionHarness;
}

export function turnUserRequest(blocks: Block[]): string {
  const user = blocks.find((block) => block.role === "user");
  return user?.text.replace(/\r\n?/g, "\n").trim() ?? "";
}

export function turnReport(blocks: Block[]): string {
  return blocks
    .filter((block) => block.role === "assistant" || block.role === "plan")
    .map((block) => block.text.replace(/\r\n?/g, "\n").trim())
    .filter(Boolean)
    .join("\n\n");
}

export function turnEditedFiles(blocks: Block[], cwd?: string): string[] {
  const files = new Map<string, string>();
  for (const block of blocks) {
    if (block.role !== "tool" && block.role !== "approval") continue;
    if (
      !isEditTool(
        block.tool?.kind,
        block.text || block.tool?.title,
        block.tool?.preview,
      )
    ) {
      continue;
    }
    const preview = block.tool?.preview;
    const path = preview?.path
      ? displayPath(preview.path, cwd)
      : preview?.fileName;
    const label = path?.trim();
    if (label) files.set(label.toLowerCase(), label);
  }
  return [...files.values()].slice(0, 40);
}

export function secondOpinionTargets(
  from: HarnessId,
  options: {
    installed: (id: HarnessId) => boolean;
    visible: (id: HarnessId) => boolean;
    probed: boolean;
  },
): HarnessId[] {
  return HARNESSES.filter((id) => {
    if (id === from) return false;
    if (!options.visible(id)) return false;
    if (!options.probed) return true;
    return options.installed(id);
  });
}

export function buildSecondOpinionPrompt(input: {
  from: HarnessId;
  userRequest: string;
  report: string;
  files: string[];
}): string {
  const fromTitle = HARNESS_TITLE[input.from];
  const request = input.userRequest.trim();
  const report = input.report.trim();
  const files = input.files.map((path) => path.trim()).filter(Boolean);

  const sections = [
    `Give a second opinion on work ${fromTitle} just finished in this same working copy. The files are already on disk.`,
    "Review that work: what is wrong, what is missing, and what you would have done differently. Fix anything you agree is broken or incomplete. If you would leave it, say so and stop. Do not redo the task from scratch unless the work is actually wrong. Read the listed files before changing anything.",
    `## User request\n${request ? limitSection(request, USER_LIMIT) : "(no user message on this turn)"}`,
  ];

  if (report) {
    sections.push(
      `## What ${fromTitle} reported\n${limitSection(report, REPORT_LIMIT)}`,
    );
  } else {
    sections.push(
      `## What ${fromTitle} reported\n(no written summary — inspect the files)`,
    );
  }

  if (files.length > 0) {
    sections.push(
      `## Files it edited\n${files.map((path) => `- ${path}`).join("\n")}`,
    );
  } else {
    sections.push("## Files it edited\n(none recorded on this turn)");
  }

  return limitSection(sections.join("\n\n"), PROMPT_LIMIT);
}

export function buildSecondOpinionCard(input: {
  from: HarnessId;
  to: HarnessId;
  userRequest: string;
  files: string[];
}): SecondOpinionMeta {
  const request = input.userRequest.replace(/\s+/g, " ").trim();
  return {
    from: input.from,
    to: input.to,
    ...(request ? { request: request.slice(0, 240) } : {}),
    ...(input.files.length > 0 ? { files: input.files.length } : {}),
  };
}
