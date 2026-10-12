import { gitCommitMessage } from "../../../platform/tauri/fs";
import { createCommitCache, MAX_CACHED_COMMITS } from "./commitCache";

export type CommitMessage = {
  /** Original Git message, including its paragraph spacing. */
  raw: string;
  /** First non-empty line; the row and the card headline already show it. */
  subject: string;
  /** Everything after the subject, trimmed. Empty when the commit has none. */
  description: string;
};

/** Alias kept so the cap reads as being about messages at the call site. */
export const MAX_CACHED_COMMIT_MESSAGES = MAX_CACHED_COMMITS;

/**
 * Module-level so every card for the same commit shares one Git call, and so
 * the identity is stable for the hook's dependency list.
 */
export const commitMessageCache = createCommitCache<CommitMessage>(
  async (cwd, sha) => splitCommitMessage(await gitCommitMessage(cwd, sha)),
);

/** Split `git show --pretty=%B` output into the subject and its description. */
export function splitCommitMessage(text: string): CommitMessage {
  const lines = text.split(/\r?\n/);
  let index = 0;
  while (index < lines.length && lines[index]!.trim() === "") index++;
  const subject = (lines[index] ?? "").trim();
  const description = lines
    .slice(index + 1)
    .join("\n")
    .trim();
  return { raw: text, subject, description };
}

/** A cached message, or undefined when it has not been loaded yet. */
export function peekCommitMessage(
  cwd: string,
  sha: string,
): CommitMessage | undefined {
  return commitMessageCache.peek(cwd, sha);
}

/** Load a commit's full message. Resolves to null when Git cannot report it. */
export function loadCommitMessage(
  cwd: string,
  sha: string,
): Promise<CommitMessage | null> {
  return commitMessageCache.load(cwd, sha);
}

/** Drop the cache, so one test cannot be served a message loaded by another. */
export function clearCommitMessageCache(): void {
  commitMessageCache.clear();
}
