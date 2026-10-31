import type { GitStatus, PlannedCommit } from "@/features/connection/useProjectGit";

/** Past this many changed files a commit plan is offered, below it a plain commit. */
export const PLAN_FROM_FILES = 4;

/** Where a working copy stands on its way from edited files to a pull request. */
export type ShipStep = "commit" | "push" | "pr" | "done";

/**
 * The next step, read from the working copy and branch rather than remembered,
 * so it holds across the desktop, another phone and a restart. None when there
 * is nothing to ship or the branch isn't known yet.
 */
export function shipStep(files: number, status: GitStatus | null): ShipStep | null {
  if (files > 0) return "commit";
  if (!status?.remote) return null;
  const unpushed = status.upstream
    ? status.ahead > 0 || status.behind > 0
    : status.ahead > 0 || status.aheadOfDefault > 0;
  if (unpushed) return "push";
  if (status.pr) return "done";
  const onDefault = !!status.branch && status.branch === status.defaultBranch;
  if (!onDefault && !!status.defaultBranch && status.aheadOfDefault > 0 && status.behind === 0)
    return "pr";
  return null;
}

/** What a step says about the work, for the card and the modal's header. */
export function shipTitle(step: ShipStep, files: number, status: GitStatus | null) {
  switch (step) {
    case "commit":
      return `${files} ${files === 1 ? "file" : "files"} changed`;
    case "push":
      return status?.upstream ? `Committed · ${status.ahead} ahead` : "Committed · not pushed";
    case "pr":
      return "Pushed · no pull request";
    case "done":
      return status?.pr ? `PR #${status.pr.number} open` : "Pull request open";
  }
}

export function syncLabel(status: GitStatus) {
  if (!status.upstream) return "Publish Branch";
  if (status.ahead > 0 && status.behind > 0) return `Sync ↓${status.behind} ↑${status.ahead}`;
  if (status.ahead > 0) return `Push ${status.ahead} ${status.ahead === 1 ? "commit" : "commits"}`;
  return `Pull ${status.behind} ${status.behind === 1 ? "commit" : "commits"}`;
}

/** The plan's commits the reader kept, with the files and lines they take. */
export function selection(plan: PlannedCommit[], on: boolean[]) {
  const kept = plan.filter((_, i) => on[i]);
  return {
    commits: kept,
    files: kept.reduce((sum, commit) => sum + commit.paths.length, 0),
    additions: kept.reduce((sum, commit) => sum + commit.additions, 0),
    deletions: kept.reduce((sum, commit) => sum + commit.deletions, 0),
  };
}
