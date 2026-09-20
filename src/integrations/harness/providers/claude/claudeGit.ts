import { gitRangeContext, gitStagedContext } from "../../../../platform/tauri/fs";
import {
  buildBranchNamePrompt,
  buildCommitMessagePrompt,
  buildPrContentPrompt,
  formatCommitMessage,
  parseBranchName,
  parseCommitMessage,
  parsePrContent,
  type PrContent,
} from "../../../../features/source-control/model/gitText";
import { runClaudeTextPrompt } from "./claudeText";

const GIT_TIMEOUT_MS = 90_000;

export async function generateClaudeCommitMessage(cwd: string): Promise<string> {
  const context = await gitStagedContext(cwd);
  const output = await runClaudeTextPrompt({
    cwd,
    prompt: buildCommitMessagePrompt({
      branch: context.branch,
      stagedSummary: context.summary,
      stagedPatch: context.patch,
    }),
    timeoutMs: GIT_TIMEOUT_MS,
  });
  const parsed = parseCommitMessage(output);
  if (parsed) return formatCommitMessage(parsed);
  const snippet = output.trim().replace(/\s+/g, " ").slice(0, 240);
  throw new Error(
    snippet
      ? `Could not generate a commit message. Model replied: ${snippet}`
      : "Could not generate a commit message. Claude Code returned no text.",
  );
}

export async function generateClaudePrContent(
  cwd: string,
): Promise<(PrContent & { base: string; head: string }) | null> {
  const range = await gitRangeContext(cwd);
  let parsed: PrContent | null = null;
  try {
    const output = await runClaudeTextPrompt({
      cwd,
      prompt: buildPrContentPrompt({
        baseBranch: range.base,
        headBranch: range.head,
        commitSummary: range.commitSummary,
        diffSummary: range.diffSummary,
        diffPatch: range.diffPatch,
      }),
      timeoutMs: GIT_TIMEOUT_MS,
    });
    parsed = parsePrContent(output);
  } catch (error) {
    console.debug("[monocode] pr content", error);
  }
  const title =
    parsed?.title ||
    range.commitSummary.split(/\r?\n/)[0]?.trim() ||
    `Update ${range.head}`;
  return {
    title,
    body: parsed?.body || range.commitSummary.trim(),
    base: range.base,
    head: range.head,
  };
}

export async function generateClaudeBranchName(
  cwd: string,
  message: string,
): Promise<string | null> {
  try {
    const output = await runClaudeTextPrompt({
      cwd,
      prompt: buildBranchNamePrompt(message),
      timeoutMs: GIT_TIMEOUT_MS,
    });
    return parseBranchName(output);
  } catch (error) {
    console.debug("[monocode] branch name", error);
    return null;
  }
}
