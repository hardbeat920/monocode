import { gitCommitMessage } from "../../../platform/tauri/fs";

export type CommitMessage = {
  /** Original Git message, including its paragraph spacing. */
  raw: string;
  /** First non-empty line; the row and the card headline already show it. */
  subject: string;
  /** Everything after the subject, trimmed. Empty when the commit has none. */
  description: string;
};

/**
 * Commit messages are immutable, so a successful lookup is cached for the
 * session. Failures are not cached: the next hover retries rather than
 * pinning a transient Git error.
 *
 * The map is capped rather than cleared per project, which bounds it no matter
 * how many repositories a session visits without needing call sites to tell it
 * when the project changed. Map iteration is insertion-ordered, so the oldest
 * entries are the ones dropped.
 */
const cache = new Map<string, CommitMessage>();
const inFlight = new Map<string, Promise<CommitMessage | null>>();

/** Roughly a full page of history per repository, several projects over. */
export const MAX_CACHED_COMMIT_MESSAGES = 256;

function cacheKey(cwd: string, sha: string): string {
  return `${cwd}\u0000${sha}`;
}

function remember(key: string, value: CommitMessage): void {
  cache.set(key, value);
  for (const stale of cache.keys()) {
    if (cache.size <= MAX_CACHED_COMMIT_MESSAGES) break;
    cache.delete(stale);
  }
}

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
  return cache.get(cacheKey(cwd, sha));
}

/**
 * Load a commit's full message, deduping concurrent requests for the same
 * commit. Resolves to null when Git cannot report the commit.
 */
export function loadCommitMessage(
  cwd: string,
  sha: string,
): Promise<CommitMessage | null> {
  const key = cacheKey(cwd, sha);
  const cached = cache.get(key);
  if (cached) return Promise.resolve(cached);

  const pending = inFlight.get(key);
  if (pending) return pending;

  const request = gitCommitMessage(cwd, sha)
    .then((text) => {
      const message = splitCommitMessage(text);
      remember(key, message);
      return message;
    })
    .catch(() => null)
    .finally(() => {
      inFlight.delete(key);
    });

  inFlight.set(key, request);
  return request;
}
