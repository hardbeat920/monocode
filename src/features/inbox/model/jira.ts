import { invoke } from "@tauri-apps/api/core";
import { recordInboxSelfActivity } from "./inboxSelfActivity";

export type JiraProject = {
  id: string;
  key: string;
  name: string;
};

export type JiraIssue = {
  provider: "jira";
  kind: "jira";
  id: string;
  identifier: string;
  number: number;
  title: string;
  url: string;
  state: string;
  /** Jira status category: `new`, `indeterminate` or `done`. */
  stateType: string;
  updatedAt: string;
  labels: { name: string; color: string }[];
  assignees: { login: string; avatarUrl?: string }[];
  draft: boolean;
  repo: string;
  teamId: string;
  teamName: string;
  projectPath: string;
};

export type JiraIssueDetails = {
  body: string;
  author: string;
  authorAvatarUrl?: string;
};

export type JiraIssueComment = {
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
  replies: JiraIssueComment[];
};

export type JiraIssueThread = {
  comments: JiraIssueComment[];
  truncated: boolean;
  reviewDecision: string;
  baseRefName: string;
  headRefName: string;
};

export type JiraStatus = {
  connected: boolean;
  site: string;
  email: string;
};

const PROJECT_IDS_KEY = "monocode.jiraHiddenProjects";
export const JIRA_CHANGE_EVENT = "monocode:jira-change";

// Keyed by issue key (ENG-42): the REST paths and browse URLs both take it.
const detailsByKey = new Map<string, JiraIssueDetails>();
const threadByKey = new Map<string, JiraIssueThread>();
const threadInflight = new Map<string, Promise<JiraIssueThread>>();

let cacheGeneration = 0;

export function clearJiraCache() {
  cacheGeneration += 1;
  detailsByKey.clear();
  threadByKey.clear();
  threadInflight.clear();
}

export function jiraConnected(): Promise<JiraStatus> {
  return invoke<JiraStatus>("jira_status");
}

export async function saveJiraConfig(config: {
  site: string;
  email: string;
  token: string;
}): Promise<JiraStatus> {
  const status = await invoke<JiraStatus>("jira_set_config", {
    site: config.site.trim(),
    email: config.email.trim(),
    token: config.token.trim(),
  });
  clearJiraCache();
  return status;
}

export async function disconnectJira(): Promise<JiraStatus> {
  const status = await invoke<JiraStatus>("jira_set_config", {
    site: "",
    email: "",
    token: "",
  });
  clearJiraCache();
  return status;
}

export function listJiraProjects(): Promise<JiraProject[]> {
  return invoke<JiraProject[]>("jira_list_projects");
}

/** `null` means do not filter by project. `[]` means every known project is hidden. */
export function jiraProjectIdsForFetch(
  projects: readonly JiraProject[],
  hiddenIds: readonly string[],
): string[] | null {
  if (hiddenIds.length === 0) return null;
  const hidden = new Set(hiddenIds);
  const visible = projects
    .filter((project) => !hidden.has(project.id))
    .map((project) => project.id);
  if (visible.length === projects.length) return null;
  return visible;
}

export function listJiraIssues(query: {
  assignedToMe: boolean;
  state: "open" | "all";
  projectIds: string[];
  limit?: number;
}): Promise<JiraIssue[]> {
  return invoke<JiraIssue[]>("jira_list_issues", {
    assignedToMe: query.assignedToMe,
    state: query.state,
    projectIds: query.projectIds,
    limit: query.limit,
  });
}

export function peekJiraIssueDetails(key: string): JiraIssueDetails | null {
  return detailsByKey.get(key) ?? null;
}

export async function jiraIssueDetails(key: string): Promise<JiraIssueDetails> {
  const generation = cacheGeneration;
  const details = await invoke<JiraIssueDetails>("jira_issue_details", {
    key,
  });
  if (generation === cacheGeneration) detailsByKey.set(key, details);
  return details;
}

export function peekJiraIssueThread(key: string): JiraIssueThread | null {
  return threadByKey.get(key) ?? null;
}

export async function jiraIssueThread(
  key: string,
  options?: { force?: boolean },
): Promise<JiraIssueThread> {
  if (options?.force) {
    threadByKey.delete(key);
    threadInflight.delete(key);
  }
  const pending = threadInflight.get(key);
  if (pending) return pending;
  const generation = cacheGeneration;
  const promise = invoke<JiraIssueThread>("jira_issue_thread", { key })
    .then((thread) => {
      if (
        generation === cacheGeneration &&
        threadInflight.get(key) === promise
      ) {
        threadByKey.set(key, thread);
      }
      return thread;
    })
    .finally(() => {
      if (threadInflight.get(key) === promise) threadInflight.delete(key);
    });
  threadInflight.set(key, promise);
  return promise;
}

export async function jiraIssueComment(
  issue: { id: string; key: string },
  body: string,
): Promise<string> {
  const url = await invoke<string>("jira_issue_comment", {
    key: issue.key,
    body: body.trim(),
  });
  threadByKey.delete(issue.key);
  threadInflight.delete(issue.key);
  recordInboxSelfActivity({ provider: "jira", kind: "jira", id: issue.id });
  return url;
}

export function loadHiddenJiraProjectIds(): string[] {
  try {
    const raw = localStorage.getItem(PROJECT_IDS_KEY);
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

export function saveHiddenJiraProjectIds(ids: string[]) {
  try {
    localStorage.setItem(PROJECT_IDS_KEY, JSON.stringify(ids));
  } catch {
    // private mode / quota
  }
  notifyJiraChange();
}

export function notifyJiraChange() {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new Event(JIRA_CHANGE_EVENT));
}
