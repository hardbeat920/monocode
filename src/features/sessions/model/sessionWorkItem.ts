import { gitPrStatus } from "../../../platform/tauri/fs";
import {
  githubRepo,
  inboxIdentityKey,
  type InboxItem,
  type GithubTaskKind,
} from "../../inbox/model/githubTasks";
import {
  linearConnected,
  linearTeamKeys,
  lookupLinearIssue,
} from "../../inbox/model/linear";
import type {
  GithubLinkedWorkItem,
  LinearLinkedWorkItem,
  LinkedWorkItem,
} from "./session";
import { numberStandsAlone, type GeneratedWorkItemHint } from "./sessionTitle";

const GITHUB_URL_RE =
  /https?:\/\/github\.com\/([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)\/(pull|issues)\/(\d+)\b/i;
/** Workspace-scoped or org-less issue URLs; a trailing title slug is ignored. */
const LINEAR_URL_RE =
  /https?:\/\/linear\.app\/(?:([A-Za-z0-9_.-]+)\/)?issue\/([A-Za-z][A-Za-z0-9]{0,9}-[1-9]\d*)\b/i;
/** A ticket key such as `ENG-42`. Also matches things like `UTF-8`, so callers verify with Linear. */
const TICKET_KEY_RE = /\b([A-Za-z][A-Za-z0-9]{0,9})-([1-9]\d*)\b/g;
const TICKET_KEY_EXACT_RE = /^([A-Z][A-Z0-9]{0,9})-([1-9]\d*)$/;
const MAX_TICKET_LOOKUPS = 5;

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
  const workspace = match[1] ? `${match[1]}/` : "";
  return {
    kind: "linear",
    ...parsed,
    url: `https://linear.app/${workspace}issue/${parsed.identifier}`,
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
    const key = `${match[1].toUpperCase()}-${Number(match[2])}`;
    if (validNumber(Number(match[2])) && !keys.includes(key)) keys.push(key);
  }
  return keys;
}

/** The persisted shape, or undefined when the value is not a usable link. */
export function normalizeLinkedWorkItem(
  value: unknown,
): LinkedWorkItem | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }
  const item = value as {
    kind?: unknown;
    repo?: unknown;
    number?: unknown;
    url?: unknown;
    identifier?: unknown;
    id?: unknown;
  };
  const kind = item.kind;
  const repo = typeof item.repo === "string" ? item.repo.trim() : "";
  if (kind === "linear") {
    const parsed = parseLinearIdentifier(
      typeof item.identifier === "string" ? item.identifier : "",
    );
    const url = typeof item.url === "string" ? item.url.trim() : "";
    // The badge shows the identifier and the URL opens the issue; they must agree.
    const fromUrl = parseLinearWorkItemUrl(url);
    if (!parsed || !fromUrl || fromUrl.identifier !== parsed.identifier) {
      return undefined;
    }
    const id = typeof item.id === "string" ? item.id.trim() : "";
    return {
      kind,
      identifier: parsed.identifier,
      ...(id ? { id } : {}),
      repo: repo || parsed.repo,
      number: parsed.number,
      url,
    };
  }
  const number = item.number;
  if (
    (kind !== "issue" && kind !== "pr") ||
    !validRepo(repo) ||
    typeof number !== "number" ||
    !validNumber(number)
  ) {
    return undefined;
  }
  return { kind, repo, number, url: githubUrl(repo, kind, number) };
}

export function linkedWorkItemsEqual(
  left: LinkedWorkItem,
  right: LinkedWorkItem,
): boolean {
  if (left.kind !== right.kind) return false;
  if (left.kind === "linear" && right.kind === "linear") {
    return (
      left.identifier === right.identifier &&
      (left.id ?? "") === (right.id ?? "") &&
      left.repo === right.repo &&
      left.number === right.number &&
      left.url === right.url
    );
  }
  return (
    left.repo === right.repo &&
    left.number === right.number &&
    left.url === right.url
  );
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

/** Keep the keys whose team prefix Linear knows. A missing team list keeps every key. */
export function ticketKeysForTeams(
  keys: readonly string[],
  teamKeys: ReadonlySet<string> | null,
): string[] {
  if (!teamKeys) return [...keys];
  return keys.filter((key) => teamKeys.has(key.slice(0, key.indexOf("-"))));
}

type TicketKeyResolution = {
  linked: LinearLinkedWorkItem | null;
  /** Numbers of the keys that looked like real Linear tickets. */
  ticketNumbers: ReadonlySet<number>;
};

const NO_TICKETS: ReadonlySet<number> = new Set();

function ticketNumbers(keys: readonly string[]): ReadonlySet<number> {
  return new Set(keys.map((key) => Number(key.slice(key.indexOf("-") + 1))));
}

/** The workspace segment of a linear.app issue URL, or null for an org-less URL. */
export function linearWorkspaceFromUrl(url: string): string | null {
  const match = LINEAR_URL_RE.exec(url);
  return match?.[1] ? match[1].toLowerCase() : null;
}

/** Confirm ticket keys against Linear; the first one that exists wins. */
async function resolveLinearTicketKey(
  keys: readonly string[],
): Promise<TicketKeyResolution> {
  if (keys.length === 0) return { linked: null, ticketNumbers: NO_TICKETS };
  try {
    if (!(await linearConnected()).connected) {
      return { linked: null, ticketNumbers: NO_TICKETS };
    }
  } catch {
    return { linked: null, ticketNumbers: NO_TICKETS };
  }
  const candidates = ticketKeysForTeams(keys, await linearTeamKeys());
  const numbers = ticketNumbers(candidates);
  for (const key of candidates.slice(0, MAX_TICKET_LOOKUPS)) {
    try {
      const linked = linkedWorkItemFromLinearIssue(
        await lookupLinearIssue(key),
      );
      if (linked) return { linked, ticketNumbers: numbers };
    } catch {
      // An unknown key such as `UTF-8`; try the next one.
    }
  }
  return { linked: null, ticketNumbers: numbers };
}

async function resolveCurrentPr(
  cwd: string,
): Promise<GithubLinkedWorkItem | null> {
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

  // Explicit GitHub references ("issue #12", "PR 42", "this PR") win over a
  // ticket key so an existing PR workflow keeps its PR link.
  const explicit = explicitHint(message);
  if (!explicit && referencesCurrentPr(message)) {
    const pr = await resolveCurrentPr(cwd);
    if (pr) return pr;
  }

  let ticketNumbersSeen: ReadonlySet<number> = NO_TICKETS;
  if (!explicit) {
    const ticket = await resolveLinearTicketKey(ticketKeysInMessage(message));
    if (ticket.linked) return ticket.linked;
    ticketNumbersSeen = ticket.ticketNumbers;
  }

  // A ticket key such as `SW-29` is the usual source of an invented GitHub
  // issue number. A model guess that only repeats a ticket number is dropped;
  // a number that also stands alone in the message, such as "#8", is kept.
  const hint =
    explicit ??
    (generatedHint &&
    (!ticketNumbersSeen.has(generatedHint.number) ||
      numberStandsAlone(message, generatedHint.number))
      ? generatedHint
      : null);
  if (!hint || !validNumber(hint.number)) return null;
  try {
    const repo = await githubRepo(cwd);
    if (!validRepo(repo)) return null;
    return { ...hint, repo, url: githubUrl(repo, hint.kind, hint.number) };
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
    // Two UUIDs settle it; identifiers can repeat across workspaces.
    if (linked.id && item.id) return linked.id === item.id;
    if ((item.identifier ?? "").trim().toUpperCase() !== linked.identifier) {
      return false;
    }
    const linkedWorkspace = linearWorkspaceFromUrl(linked.url);
    const itemWorkspace = linearWorkspaceFromUrl(item.url ?? "");
    return (
      !linkedWorkspace || !itemWorkspace || linkedWorkspace === itemWorkspace
    );
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
