import type { InboxAction, InboxDetail, InboxItemAction } from "./protocol";

export type MergeAction = Extract<InboxAction, "merge" | "squash" | "rebase">;

const MERGES: { action: MergeAction; label: string }[] = [
  { action: "merge", label: "Create a merge commit" },
  { action: "squash", label: "Squash and merge" },
  { action: "rebase", label: "Rebase and merge" },
];

/** The ways this pull request can be merged now. */
export function mergeOptions(actions: readonly InboxItemAction[]) {
  return MERGES.filter(({ action }) => actions.includes(action));
}

export type ActionCopy = {
  title: string;
  detail: string;
  /** The button that goes ahead. */
  confirm: string;
  /** The title when the provider refuses. */
  failed: string;
};

/** What the phone asks before an action, in the desktop's words. */
export function actionCopy(
  action: InboxAction,
  branches: Pick<InboxDetail, "baseRef" | "headRef">,
): ActionCopy {
  const source = branches.headRef ? `“${branches.headRef}”` : "this branch";
  const destination = branches.baseRef ? `“${branches.baseRef}”` : "the base branch";
  switch (action) {
    case "merge":
      return {
        title: "Merge this pull request?",
        detail: `Every commit from ${source} will be added to ${destination} with a merge commit.`,
        confirm: "Merge pull request",
        failed: "Couldn’t merge",
      };
    case "squash":
      return {
        title: "Squash and merge?",
        detail: `The commits from ${source} will be combined into one commit on ${destination}.`,
        confirm: "Squash and merge",
        failed: "Couldn’t merge",
      };
    case "rebase":
      return {
        title: "Rebase and merge?",
        detail: `The commits from ${source} will be rebased individually onto ${destination}.`,
        confirm: "Rebase and merge",
        failed: "Couldn’t merge",
      };
    case "close":
      return {
        title: "Close this pull request?",
        detail: "The pull request will close without merging. You can reopen it later.",
        confirm: "Close pull request",
        failed: "Couldn’t close",
      };
    case "reopen":
      return {
        title: "Reopen this pull request?",
        detail: "The pull request will return to the open state.",
        confirm: "Reopen pull request",
        failed: "Couldn’t reopen",
      };
    case "ready":
      return {
        title: "Mark as ready for review?",
        detail: "Reviewers will see that this pull request is ready for feedback.",
        confirm: "Ready for review",
        failed: "Couldn’t update",
      };
    case "draft":
      return {
        title: "Convert to draft?",
        detail: "Reviewers will see that this pull request is not ready to merge.",
        confirm: "Convert to draft",
        failed: "Couldn’t convert",
      };
  }
}

/** What the toast says once the provider took the action. */
export function actionMessage(action: InboxAction, queued: boolean) {
  if (queued) return "Merge queued or auto-merge enabled";
  switch (action) {
    case "merge":
    case "squash":
    case "rebase":
      return "Pull request merged";
    case "close":
      return "Pull request closed";
    case "reopen":
      return "Pull request reopened";
    case "ready":
      return "Ready for review";
    case "draft":
      return "Converted to draft";
  }
}
