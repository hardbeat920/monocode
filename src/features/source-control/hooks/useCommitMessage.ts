import { useCommitData } from "./useCommitData";
import { commitMessageCache, type CommitMessage } from "../model/commitMessage";

/** Lazily loads a commit's full message for the hover card. */
export function useCommitMessage(
  cwd: string,
  sha: string,
): CommitMessage | null | undefined {
  return useCommitData(cwd, sha, commitMessageCache);
}
