import { gitPrStatus } from "./fs";
import {
  githubRepo,
  inboxIdentityKey,
  type InboxItem,
  type GithubTaskKind,
} from "./githubTasks";
import type { LinkedWorkItem } from "./session";
import type { GeneratedWorkItemHint } from "./sessionTitle";

const GITHUB_URL_RE =
  /https?:\/\/github\.com\/([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)\/(pull|issues)\/(\d+)\b/i;

function validNumber(value: number): boolean {
  return Number.isSafeInteger(value) && value > 0;
}

function githubUrl(repo: string, kind: GithubTaskKind, number: number): string {
  return `https://github.com/${repo}/${kind === "pr" ? "pull" : "issues"}/${number}`;
}

export function parseGithubWorkItemUrl(message: string): LinkedWorkItem | null {
  const match = GITHUB_URL_RE.exec(message);
  if (!match) return null;
  const number = Number(match[4]);
  if (!validNumber(number)) return null;
  const repo = `${match[1]}/${match[2]}`;
  const kind = match[3].toLowerCase() === "pull" ? "pr" : "issue";
  return { kind, repo, number, url: githubUrl(repo, kind, number) };
}

function explicitHint(message: string): GeneratedWorkItemHint | null {
  const patterns: Array<[GithubTaskKind, RegExp]> = [
    ["pr", /\b(?:pr|pull\s+request)\s*#?\s*(\d+)\b/i],
    ["issue", /\bissue\s*#?\s*(\d+)\b/i],
  ];
  for (const [kind, pattern] of patterns) {
    const match = pattern.exec(message);
    const number = Number(match?.[1]);
    if (match && validNumber(number)) return { kind, number };
  }
  return null;
}

function validRepo(repo: string): boolean {
  return /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repo);
}

function repoFromGithubUrl(url: string): string | null {
  const match = GITHUB_URL_RE.exec(url);
  return match ? `${match[1]}/${match[2]}` : null;
}

function referencesCurrentPr(message: string): boolean {
  return /\b(?:this|the|current)\s+(?:pr|pull\s+request)\b/i.test(message);
}

/** Resolve explicit first-message context to one stable GitHub identity. */
export async function resolveLinkedWorkItem(
  message: string,
  cwd: string,
  generatedHint: GeneratedWorkItemHint | null,
): Promise<LinkedWorkItem | null> {
  const fromUrl = parseGithubWorkItemUrl(message);
  if (fromUrl) return fromUrl;

  const hint = explicitHint(message) ?? generatedHint;
  if (hint && validNumber(hint.number)) {
    try {
      const repo = await githubRepo(cwd);
      if (!validRepo(repo)) return null;
      return {
        ...hint,
        repo,
        url: githubUrl(repo, hint.kind, hint.number),
      };
    } catch {
      return null;
    }
  }

  if (!referencesCurrentPr(message)) return null;
  try {
    const pr = await gitPrStatus(cwd);
    if (!pr || !validNumber(pr.number)) return null;
    const repo = repoFromGithubUrl(pr.url) ?? (await githubRepo(cwd));
    if (!validRepo(repo)) return null;
    return {
      kind: "pr",
      repo,
      number: pr.number,
      url: pr.url || githubUrl(repo, "pr", pr.number),
    };
  } catch {
    return null;
  }
}

export function linkedWorkItemFromInboxItem(
  item: InboxItem,
): LinkedWorkItem | null {
  if (
    item.provider !== "github" ||
    (item.kind !== "issue" && item.kind !== "pr") ||
    !validNumber(item.number) ||
    !validRepo(item.repo)
  ) {
    return null;
  }
  return {
    kind: item.kind,
    repo: item.repo,
    number: item.number,
    url: item.url || githubUrl(item.repo, item.kind, item.number),
  };
}

export function inboxItemMatchesLinkedWorkItem(
  item: InboxItem,
  linked: LinkedWorkItem,
): boolean {
  return (
    item.provider === "github" &&
    item.kind === linked.kind &&
    item.number === linked.number &&
    item.repo.trim().toLowerCase() === linked.repo.trim().toLowerCase()
  );
}

/** Same key used by Inbox selection, without synthesizing a full Inbox item. */
export function linkedWorkItemInboxKey(linked: LinkedWorkItem): string {
  return `github:${inboxIdentityKey(linked)}`;
}

/** Find local sessions whose persisted GitHub identity matches an Inbox row. */
export function relatedSessionsForInboxItem<
  T extends { linkedWorkItem?: LinkedWorkItem },
>(item: InboxItem, sessions: readonly T[]): T[] {
  if (item.provider !== "github") return [];
  return sessions.filter(
    (session) =>
      session.linkedWorkItem != null &&
      inboxItemMatchesLinkedWorkItem(item, session.linkedWorkItem),
  );
}
