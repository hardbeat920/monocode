import { gitPrStatus } from "../../../platform/tauri/fs";
import {
  githubRepo,
  inboxIdentityKey,
  type InboxItem,
  type GithubTaskKind,
} from "../../inbox/model/githubTasks";
import { linearConnected, lookupLinearIssue } from "../../inbox/model/linear";
import type {
  GithubLinkedWorkItem,
  LinearLinkedWorkItem,
  LinkedWorkItem,
} from "./session";
import type { GeneratedWorkItemHint } from "./sessionTitle";

const GITHUB_URL_RE =
  /https?:\/\/github\.com\/([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)\/(pull|issues)\/(\d+)\b/i;
const LINEAR_URL_RE =
  /https?:\/\/linear\.app\/([A-Za-z0-9_.-]+)\/issue\/([A-Za-z][A-Za-z0-9]{0,6}-\d+)\b/i;
/** A ticket key such as `ENG-42`. Also matches things like `UTF-8`, so callers verify with Linear. */
const TICKET_KEY_RE = /\b([A-Z][A-Z0-9]{0,6})-(\d+)\b/g;
const TICKET_KEY_EXACT_RE = /^([A-Z][A-Z0-9]{0,6})-(\d+)$/;
const MAX_TICKET_LOOKUPS = 3;

function validNumber(value: number): boolean {
  return Number.isSafeInteger(value) && value > 0;
}

function githubUrl(repo: string, kind: GithubTaskKind, number: number): string {
  return `https://github.com/${repo}/${kind === "pr" ? "pull" : "issues"}/${number}`;
}

export function parseGithubWorkItemUrl(
  message: string,
): GithubLinkedWorkItem | null {
  const match = GITHUB_URL_RE.exec(message);
  if (!match) return null;
  const number = Number(match[4]);
  if (!validNumber(number)) return null;
  const repo = `${match[1]}/${match[2]}`;
  const kind = match[3].toLowerCase() === "pull" ? "pr" : "issue";
  return { kind, repo, number, url: githubUrl(repo, kind, number) };
}

/** Split `ENG-42` into its team key and number. Returns null for anything else. */
export function parseLinearIdentifier(
  value: string,
): { identifier: string; repo: string; number: number } | null {
  const match = TICKET_KEY_EXACT_RE.exec(value.trim().toUpperCase());
  if (!match) return null;
  const number = Number(match[2]);
  if (!validNumber(number)) return null;
  return { identifier: `${match[1]}-${number}`, repo: match[1], number };
}

export function parseLinearWorkItemUrl(
  message: string,
): LinearLinkedWorkItem | null {
  const match = LINEAR_URL_RE.exec(message);
  if (!match) return null;
  const parsed = parseLinearIdentifier(match[2]);
  if (!parsed) return null;
  return {
    kind: "linear",
    ...parsed,
    url: `https://linear.app/${match[1]}/issue/${parsed.identifier}`,
  };
}

/** A GitHub or Linear URL pasted by the user. */
export function parseWorkItemUrl(message: string): LinkedWorkItem | null {
  return parseGithubWorkItemUrl(message) ?? parseLinearWorkItemUrl(message);
}

/** Distinct ticket keys in the message, in order of appearance. */
export function ticketKeysInMessage(message: string): string[] {
  const keys: string[] = [];
  for (const match of message.matchAll(TICKET_KEY_RE)) {
    const key = `${match[1]}-${Number(match[2])}`;
    if (validNumber(Number(match[2])) && !keys.includes(key)) keys.push(key);
  }
  return keys;
}

export function linkedWorkItemFromLinearIssue(issue: {
  id?: string;
  identifier: string;
  number: number;
  repo: string;
  url: string;
}): LinearLinkedWorkItem | null {
  const parsed = parseLinearIdentifier(issue.identifier);
  if (!parsed || !issue.url) return null;
  return {
    kind: "linear",
    identifier: parsed.identifier,
    ...(issue.id ? { id: issue.id } : {}),
    repo: issue.repo || parsed.repo,
    number: validNumber(issue.number) ? issue.number : parsed.number,
    url: issue.url,
  };
}

/** Confirm ticket keys against Linear; the first one that exists wins. */
async function resolveLinearTicketKey(
  keys: readonly string[],
): Promise<LinearLinkedWorkItem | null> {
  if (keys.length === 0) return null;
  try {
    if (!(await linearConnected()).connected) return null;
  } catch {
    return null;
  }
  for (const key of keys.slice(0, MAX_TICKET_LOOKUPS)) {
    try {
      const linked = linkedWorkItemFromLinearIssue(
        await lookupLinearIssue(key),
      );
      if (linked) return linked;
    } catch {
      // An unknown key such as `UTF-8`; try the next one.
    }
  }
  return null;
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

/** Resolve explicit first-message context to one stable GitHub or Linear identity. */
export async function resolveLinkedWorkItem(
  message: string,
  cwd: string,
  generatedHint: GeneratedWorkItemHint | null,
): Promise<LinkedWorkItem | null> {
  const fromUrl = parseWorkItemUrl(message);
  if (fromUrl) return fromUrl;

  const explicit = explicitHint(message);
  const ticketKeys = ticketKeysInMessage(message);
  if (!explicit) {
    const linear = await resolveLinearTicketKey(ticketKeys);
    if (linear) return linear;
  }

  // A ticket key such as `SW-29` is the usual source of an invented GitHub
  // issue number, so the model's guess is ignored whenever one is present.
  const hint = explicit ?? (ticketKeys.length > 0 ? null : generatedHint);
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
  if (item.provider === "linear") {
    return linkedWorkItemFromLinearIssue({
      id: item.id,
      identifier: item.identifier ?? "",
      number: item.number,
      repo: item.repo,
      url: item.url,
    });
  }
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

export function linkedWorkItemProvider(
  linked: LinkedWorkItem,
): "github" | "linear" {
  return linked.kind === "linear" ? "linear" : "github";
}

export function inboxItemMatchesLinkedWorkItem(
  item: InboxItem,
  linked: LinkedWorkItem,
): boolean {
  if (linked.kind === "linear") {
    if (item.provider !== "linear") return false;
    if (linked.id && item.id && linked.id === item.id) return true;
    return (item.identifier ?? "").trim().toUpperCase() === linked.identifier;
  }
  return (
    item.provider === "github" &&
    item.kind === linked.kind &&
    item.number === linked.number &&
    item.repo.trim().toLowerCase() === linked.repo.trim().toLowerCase()
  );
}

/** Same key used by Inbox selection, without synthesizing a full Inbox item. */
export function linkedWorkItemInboxKey(linked: LinkedWorkItem): string {
  const provider = linkedWorkItemProvider(linked);
  return `${provider}:${inboxIdentityKey({ ...linked, provider })}`;
}

/** Find local sessions whose persisted work item identity matches an Inbox row. */
export function relatedSessionsForInboxItem<
  T extends { linkedWorkItem?: LinkedWorkItem },
>(item: InboxItem, sessions: readonly T[]): T[] {
  if (item.provider !== "github" && item.provider !== "linear") return [];
  return sessions.filter(
    (session) =>
      session.linkedWorkItem != null &&
      inboxItemMatchesLinkedWorkItem(item, session.linkedWorkItem),
  );
}
