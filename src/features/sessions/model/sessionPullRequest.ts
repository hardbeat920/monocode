import { gitBranchPr } from "../../../platform/tauri/fs";
import { toolCommand } from "./monocodeToolCall";
import type { Block, LinkedWorkItem } from "./session";
import { parseGithubWorkItemUrl } from "./sessionWorkItem";

/** A branch this session pushed. No branch means whatever was checked out. */
export type PushedBranch = { branch?: string; remote?: string };

const PULL_URL_RE =
  /https?:\/\/github\.com\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/pull\/\d+/gi;

// Checking only the latest few keeps a long session to a couple of gh calls.
const MAX_PUSHED_LOOKUPS = 3;

function ranShell(block: Block): boolean {
  return block.role === "tool" && block.tool?.status !== "failed";
}

/** Plain words of each simple command; quoting is loose, which is enough here. */
function commandSegments(command: string): string[][] {
  return command
    .split(/&&|\|\||[;|\n]/)
    .map((segment) =>
      segment
        .trim()
        .split(/\s+/)
        .map((word) => word.replace(/^['"]|['"]$/g, ""))
        .filter(Boolean),
    )
    .filter((words) => words.length > 0);
}

function remoteBranch(ref: string): string | undefined {
  const target = ref.replace(/^\+/, "").split(":").pop() ?? "";
  const branch = target.replace(/^refs\/heads\//, "");
  return branch && branch !== "HEAD" && !branch.startsWith("refs/")
    ? branch
    : undefined;
}

/** `git push` targets: the remote and branches named, else the checked-out one. */
function pushTargets(words: string[]): PushedBranch[] | undefined {
  const git = words.findIndex((word) => /(?:^|\/)git$/.test(word));
  if (git < 0) return undefined;
  // The subcommand is the first word after git's own options, so
  // `git log --grep push` is not a push.
  let push = git + 1;
  while (words[push]?.startsWith("-")) {
    push += words[push] === "-C" || words[push] === "-c" ? 2 : 1;
  }
  if (words[push] !== "push") return undefined;
  const args: string[] = [];
  for (let index = push + 1; index < words.length; index += 1) {
    const word = words[index];
    if (["-d", "--delete", "-n", "--dry-run"].includes(word)) return [];
    if (word === "-o" || word === "--push-option") {
      index += 1;
      continue;
    }
    if (word.startsWith("-") || /[<>]/.test(word)) continue;
    args.push(word);
  }
  const [remote, ...refs] = args;
  const branches = refs.map(remoteBranch);
  if (branches.length === 0 || branches.every((branch) => !branch)) {
    return [{ ...(remote ? { remote } : {}) }];
  }
  return branches
    .filter((branch): branch is string => Boolean(branch))
    .map((branch) => ({ branch, ...(remote ? { remote } : {}) }));
}

/** Branches git reported updating, e.g. ` * [new branch]  a -> b`. */
function reportedBranches(output: string): string[] {
  const branches: string[] = [];
  for (const line of output.split("\n")) {
    const match = /\s->\s+(\S+)/.exec(line);
    const branch = match ? remoteBranch(match[1]) : undefined;
    if (branch) branches.push(branch);
  }
  return branches;
}

/** The last PR a successful `gh pr create` in this session printed. */
export function createdPullRequest(blocks: Block[]): LinkedWorkItem | null {
  for (let index = blocks.length - 1; index >= 0; index -= 1) {
    const block = blocks[index];
    if (!ranShell(block)) continue;
    const command = toolCommand(block);
    if (!command || !/\bgh\s+pr\s+create\b/.test(command)) continue;
    const urls = block.tool?.detail?.match(PULL_URL_RE) ?? [];
    for (let at = urls.length - 1; at >= 0; at -= 1) {
      const item = parseGithubWorkItemUrl(urls[at]);
      if (item?.kind === "pr") return item;
    }
  }
  return null;
}

/** Branches this session pushed, most recent first and without repeats. */
export function pushedBranches(blocks: Block[]): PushedBranch[] {
  const pushed: PushedBranch[] = [];
  const seen = new Set<string>();
  for (let index = blocks.length - 1; index >= 0; index -= 1) {
    const block = blocks[index];
    if (!ranShell(block)) continue;
    const command = toolCommand(block);
    if (!command || !/\bgit\b/.test(command) || !/\bpush\b/.test(command)) {
      continue;
    }
    for (const words of commandSegments(command)) {
      const targets = pushTargets(words);
      if (!targets) continue;
      const remote = targets[0]?.remote;
      const reported = reportedBranches(block.tool?.detail ?? "");
      const found = reported.length
        ? reported.map((branch) => ({ branch, ...(remote ? { remote } : {}) }))
        : targets;
      for (const target of found) {
        const key = `${target.remote ?? ""}\0${target.branch ?? ""}`;
        if (seen.has(key)) continue;
        seen.add(key);
        pushed.push(target);
      }
    }
  }
  return pushed;
}

/**
 * The PR this session produced, if it has none linked yet: one it created,
 * or one on a branch it pushed (or on its own worktree's branch), whenever
 * that PR was opened.
 */
export async function findSessionPullRequest(
  session: { blocks: Block[]; linkedWorkItem?: LinkedWorkItem },
  workCwd: string,
  options: { dedicatedWorktree: boolean },
): Promise<LinkedWorkItem | null> {
  if (session.linkedWorkItem) return null;
  const created = createdPullRequest(session.blocks);
  if (created) return created;
  const targets = pushedBranches(session.blocks).slice(0, MAX_PUSHED_LOOKUPS);
  if (options.dedicatedWorktree && !targets.some((target) => !target.branch)) {
    targets.push({});
  }
  for (const target of targets) {
    const pr = await gitBranchPr(workCwd, target.branch, target.remote).catch(
      () => null,
    );
    const item = pr ? parseGithubWorkItemUrl(pr.url) : null;
    if (item?.kind === "pr") return item;
  }
  return null;
}
