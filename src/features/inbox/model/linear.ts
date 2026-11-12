import { invoke } from "@tauri-apps/api/core";
import { recordInboxSelfActivity } from "./inboxSelfActivity";

export type LinearTeam = {
  id: string;
  key: string;
  name: string;
};

export type LinearIssue = {
  provider: "linear";
  kind: "linear";
  id: string;
  identifier: string;
  number: number;
  title: string;
  url: string;
  state: string;
  stateType: string;
  updatedAt: string;
  labels: { name: string; color: string }[];
  assignees: { login: string; avatarUrl?: string }[];
  draft: boolean;
  repo: string;
  teamId: string;
  teamName: string;
  projectId: string;
  projectName: string;
  projectPath: string;
};

export type LinearIssueDetails = {
  body: string;
  author: string;
  authorAvatarUrl?: string;
};

export type LinearIssueComment = {
  id: string;
  kind: string;
  author: string;
  authorAvatarUrl?: string;
  body: string;
  createdAt: string;
  url: string;
  state: string;
  path: string;
  line: number | null;
  resolved: boolean;
  threadId: string;
  replies: LinearIssueComment[];
};

export type LinearIssueThread = {
  comments: LinearIssueComment[];
  truncated: boolean;
  reviewDecision: string;
  baseRefName: string;
  headRefName: string;
};

export type LinearStatus = {
  connected: boolean;
};

const TEAM_IDS_KEY = "monocode.linearHiddenTeams";
export const LINEAR_CHANGE_EVENT = "monocode:linear-change";

const detailsById = new Map<string, LinearIssueDetails>();
const threadById = new Map<string, LinearIssueThread>();
const threadInflight = new Map<string, Promise<LinearIssueThread>>();

export function linearConnected(): Promise<LinearStatus> {
  return invoke<LinearStatus>("linear_status");
}

export function saveLinearToken(token: string): Promise<LinearStatus> {
  return invoke<LinearStatus>("linear_set_token", { token: token.trim() });
}

export function disconnectLinear(): Promise<LinearStatus> {
  return invoke<LinearStatus>("linear_set_token", { token: "" });
}

export function listLinearTeams(): Promise<LinearTeam[]> {
  return invoke<LinearTeam[]>("linear_list_teams");
}

const TEAM_KEYS_TTL_MS = 5 * 60_000;
let teamKeysCache: { keys: Set<string>; fetchedAt: number } | null = null;

export function clearLinearTeamKeysCache() {
  teamKeysCache = null;
}

/** Upper-case team keys such as `ENG`, cached briefly. `null` when the list is unavailable. */
export async function linearTeamKeys(): Promise<Set<string> | null> {
  if (
    teamKeysCache &&
    Date.now() - teamKeysCache.fetchedAt < TEAM_KEYS_TTL_MS
  ) {
    return teamKeysCache.keys;
  }
  const generation = cacheGeneration;
  try {
    const keys = new Set(
      (await listLinearTeams())
        .map((team) => team.key.trim().toUpperCase())
        .filter(Boolean),
    );
    if (keys.size === 0) return null;
    if (generation === cacheGeneration) {
      teamKeysCache = { keys, fetchedAt: Date.now() };
    }
    return keys;
  } catch {
    return null;
  }
}

/** `null` means do not filter by team. `[]` means every known team is hidden. */
export function linearTeamIdsForFetch(
  teams: readonly LinearTeam[],
  hiddenIds: readonly string[],
): string[] | null {
  if (hiddenIds.length === 0) return null;
  const hidden = new Set(hiddenIds);
  const visible = teams
    .filter((team) => !hidden.has(team.id))
    .map((team) => team.id);
  if (visible.length === teams.length) return null;
  return visible;
}

export function listLinearIssues(query: {
  assignedToMe: boolean;
  state: "open" | "all";
  teamIds: string[];
  limit?: number;
}): Promise<LinearIssue[]> {
  return invoke<LinearIssue[]>("linear_list_issues", {
    assignedToMe: query.assignedToMe,
    state: query.state,
    teamIds: query.teamIds,
    limit: query.limit,
  });
}

const issueByKey = new Map<string, LinearIssue>();
/** Bumped on every invalidation so a request started under an old token cannot refill the caches. */
let cacheGeneration = 0;

function issueLookupKey(key: string): string {
  return key.trim().toLowerCase();
}

export function clearLinearIssueCache() {
  cacheGeneration += 1;
  issueByKey.clear();
  clearLinearTeamKeysCache();
}

/** `key` is a Linear issue UUID or a `TEAM-123` identifier. */
export function peekLinearIssue(key: string): LinearIssue | null {
  return issueByKey.get(issueLookupKey(key)) ?? null;
}

export async function lookupLinearIssue(key: string): Promise<LinearIssue> {
  const generation = cacheGeneration;
  const issue = await invoke<LinearIssue>("linear_issue_lookup", {
    key: key.trim(),
  });
  if (generation !== cacheGeneration) return issue;
  issueByKey.set(issueLookupKey(key), issue);
  if (issue.identifier) issueByKey.set(issueLookupKey(issue.identifier), issue);
  if (issue.id) issueByKey.set(issueLookupKey(issue.id), issue);
  return issue;
}

export function peekLinearIssueDetails(id: string): LinearIssueDetails | null {
  return detailsById.get(id) ?? null;
}

export async function linearIssueDetails(
  id: string,
): Promise<LinearIssueDetails> {
  const details = await invoke<LinearIssueDetails>("linear_issue_details", {
    id,
  });
  detailsById.set(id, details);
  return details;
}

export function peekLinearIssueThread(id: string): LinearIssueThread | null {
  return threadById.get(id) ?? null;
}

export async function linearIssueThread(
  id: string,
  options?: { force?: boolean },
): Promise<LinearIssueThread> {
  if (options?.force) {
    threadById.delete(id);
    threadInflight.delete(id);
  }
  const pending = threadInflight.get(id);
  if (pending) return pending;
  const promise = invoke<LinearIssueThread>("linear_issue_thread", { id })
    .then((thread) => {
      threadById.set(id, thread);
      return thread;
    })
    .finally(() => {
      if (threadInflight.get(id) === promise) threadInflight.delete(id);
    });
  threadInflight.set(id, promise);
  return promise;
}

export async function linearIssueComment(
  id: string,
  body: string,
  options?: { parentId?: string },
): Promise<string> {
  const url = await invoke<string>("linear_issue_comment", {
    id,
    body: body.trim(),
    parentId: options?.parentId?.trim() ?? "",
  });
  threadById.delete(id);
  threadInflight.delete(id);
  recordInboxSelfActivity({ provider: "linear", kind: "linear", id });
  return url;
}

export function loadHiddenLinearTeamIds(): string[] {
  try {
    const raw = localStorage.getItem(TEAM_IDS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (id): id is string => typeof id === "string" && id.length > 0,
    );
  } catch {
    return [];
  }
}

export function saveHiddenLinearTeamIds(ids: string[]) {
  try {
    localStorage.setItem(TEAM_IDS_KEY, JSON.stringify(ids));
  } catch {
    // private mode / quota
  }
  notifyLinearChange();
}

export function notifyLinearChange() {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new Event(LINEAR_CHANGE_EVENT));
}
