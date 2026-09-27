import { gitPrStatus } from "../../../platform/tauri/fs";
import {
  githubRepo,
  inboxIdentityKey,
  type InboxItem,
  type GithubTaskKind,
} from "../../inbox/model/githubTasks";
import type { LinkedWorkItem } from "./session";
import type { GeneratedWorkItemHint } from "./sessionTitle";

const GITHUB_URL_RE =
  /https?:\/\/github\.com\/([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)\/(pull|issues)\/(\d+)\b/i;
const GITHUB_URL_TOKEN_RE =
  /^https?:\/\/github\.com\/([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)\/(pull|issues)\/(\d+)(?:[/?#]\S*)?$/i;

function validNumber(value: number): boolean {
  return Number.isSafeInteger(value) && value > 0;
}

function githubUrl(repo: string, kind: GithubTaskKind, number: number): string {
  return `https://github.com/${repo}/${kind === "pr" ? "pull" : "issues"}/${number}`;
}

function linkedWorkItemFromMatch(
  owner: string,
  name: string,
  kindPath: string,
  numberText: string,
): LinkedWorkItem | null {
  const number = Number(numberText);
  if (!validNumber(number)) return null;
  const repo = `${owner}/${name}`;
  const kind = kindPath.toLowerCase() === "pull" ? "pr" : "issue";
  return { kind, repo, number, url: githubUrl(repo, kind, number) };
}

export function parseGithubWorkItemUrl(message: string): LinkedWorkItem | null {
  const match = GITHUB_URL_RE.exec(message);
  if (!match) return null;
  return linkedWorkItemFromMatch(match[1], match[2], match[3], match[4]);
}

export function linkedWorkItemKey(item: LinkedWorkItem): string {
  return `${item.repo.trim().toLowerCase()}:${item.kind}:${item.number}`;
}

export function dedupeLinkedWorkItems(
  items: readonly LinkedWorkItem[],
): LinkedWorkItem[] {
  const seen = new Set<string>();
  const unique: LinkedWorkItem[] = [];
  for (const item of items) {
    const key = linkedWorkItemKey(item);
    if (seen.has(key)) continue;
    seen.add(key);
    unique.push(item);
  }
  return unique;
}

/** Parse comma-, space-, or newline-separated GitHub issue/PR URLs. */
export function parseGithubWorkItemUrls(
  text: string,
): LinkedWorkItem[] | null {
  const tokens = text
    .split(/[\s,;]+/)
    .map((token) => token.trim())
    .filter(Boolean);
  if (tokens.length === 0) return [];
  const items: LinkedWorkItem[] = [];
  for (const token of tokens) {
    const match = GITHUB_URL_TOKEN_RE.exec(token);
    if (!match) return null;
    const item = linkedWorkItemFromMatch(
      match[1],
      match[2],
      match[3],
      match[4],
    );
    if (!item) return null;
    items.push(item);
  }
  return dedupeLinkedWorkItems(items);
}

export function formatGithubWorkItemUrls(
  items: readonly LinkedWorkItem[],
): string {
  return items.map((item) => item.url).join(", ");
}

export function sessionLinkedWorkItems(session: {
  linkedWorkItem?: LinkedWorkItem;
  linkedWorkItems?: readonly LinkedWorkItem[];
} | null | undefined): LinkedWorkItem[] {
  if (!session) return [];
  if (session.linkedWorkItems && session.linkedWorkItems.length > 0) {
    return [...session.linkedWorkItems];
  }
  return session.linkedWorkItem ? [session.linkedWorkItem] : [];
}

export function linkedWorkItemFields(
  items: readonly LinkedWorkItem[] | undefined,
): {
  linkedWorkItem?: LinkedWorkItem;
  linkedWorkItems?: LinkedWorkItem[];
} {
  const list = items?.length ? dedupeLinkedWorkItems(items) : [];
  if (list.length === 0) return {};
  if (list.length === 1) return { linkedWorkItem: list[0] };
  return { linkedWorkItem: list[0], linkedWorkItems: list };
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

/** Restore the GitHub identity persisted on an event-triggered automation run. */
export function linkedWorkItemFromAutomationEvent(run: {
  trigger: string;
  eventKind?: string;
  eventKey?: string;
}): LinkedWorkItem | null {
  if (run.trigger !== "event" || run.eventKind !== "github") return null;
  const match = /^github:(pr|issue):([^/:]+\/[^/:]+):([1-9]\d*)$/i.exec(
    run.eventKey?.trim() ?? "",
  );
  if (!match) return null;
  const number = Number(match[3]);
  if (!validNumber(number)) return null;
  const kind = match[1].toLowerCase() === "pr" ? "pr" : "issue";
  const repo = match[2];
  if (!validRepo(repo)) return null;
  return { kind, repo, number, url: githubUrl(repo, kind, number) };
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
  T extends {
    linkedWorkItem?: LinkedWorkItem;
    linkedWorkItems?: readonly LinkedWorkItem[];
  },
>(item: InboxItem, sessions: readonly T[]): T[] {
  if (item.provider !== "github") return [];
  return sessions.filter((session) =>
    sessionLinkedWorkItems(session).some((linked) =>
      inboxItemMatchesLinkedWorkItem(item, linked),
    ),
  );
}
