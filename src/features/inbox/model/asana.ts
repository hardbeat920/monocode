import { invoke } from "@tauri-apps/api/core";
import {
  normalizeProjectPath,
  sameProjectPath,
} from "../../projects/model/recents";
import { recordInboxSelfActivity } from "./inboxSelfActivity";

export type AsanaProject = {
  id: string;
  /** Workspace name: project names are only unique within a workspace. */
  key: string;
  name: string;
};

export type AsanaIssue = {
  provider: "asana";
  kind: "asana";
  id: string;
  identifier: string;
  number: number;
  title: string;
  url: string;
  state: string;
  /** `done` for completed tasks, `new` otherwise. */
  stateType: string;
  updatedAt: string;
  /** `YYYY-MM-DD`, empty when the task has no due date. */
  dueOn: string;
  labels: { name: string; color: string }[];
  assignees: { login: string; avatarUrl?: string }[];
  draft: boolean;
  repo: string;
  teamId: string;
  teamName: string;
  projectPath: string;
  /** Every Asana project the task belongs to. */
  projects: AsanaProjectRef[];
};

export type AsanaProjectRef = {
  id: string;
  name: string;
};

/** Asana project gid → local MonoCode project path. */
export type AsanaProjectLinks = Record<string, string>;

export type AsanaIssueDetails = {
  body: string;
  author: string;
  authorAvatarUrl?: string;
};

export type AsanaIssueComment = {
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
  replies: AsanaIssueComment[];
};

export type AsanaIssueThread = {
  comments: AsanaIssueComment[];
  truncated: boolean;
  reviewDecision: string;
  baseRefName: string;
  headRefName: string;
};

export type AsanaStatus = {
  connected: boolean;
  name: string;
  email: string;
};

const PROJECT_IDS_KEY = "monocode.asanaHiddenProjects";
const PROJECT_LINKS_KEY = "monocode.asanaProjectLinks";
export const ASANA_CHANGE_EVENT = "monocode:asana-change";

const detailsById = new Map<string, AsanaIssueDetails>();
const threadById = new Map<string, AsanaIssueThread>();
const threadInflight = new Map<string, Promise<AsanaIssueThread>>();

let cacheGeneration = 0;

export function clearAsanaCache() {
  cacheGeneration += 1;
  detailsById.clear();
  threadById.clear();
  threadInflight.clear();
}

export function asanaConnected(): Promise<AsanaStatus> {
  return invoke<AsanaStatus>("asana_status");
}

export async function saveAsanaToken(token: string): Promise<AsanaStatus> {
  const status = await invoke<AsanaStatus>("asana_set_token", {
    token: token.trim(),
  });
  clearAsanaCache();
  return status;
}

export async function disconnectAsana(): Promise<AsanaStatus> {
  const status = await invoke<AsanaStatus>("asana_set_token", { token: "" });
  clearAsanaCache();
  return status;
}

export function listAsanaProjects(): Promise<AsanaProject[]> {
  return invoke<AsanaProject[]>("asana_list_projects");
}

/** `null` means do not filter by project. `[]` means every known project is hidden. */
export function asanaProjectIdsForFetch(
  projects: readonly AsanaProject[],
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

export function listAsanaIssues(query: {
  assignedToMe: boolean;
  state: "open" | "all";
  projectIds: string[];
  limit?: number;
}): Promise<AsanaIssue[]> {
  return invoke<AsanaIssue[]>("asana_list_issues", {
    assignedToMe: query.assignedToMe,
    state: query.state,
    projectIds: query.projectIds,
    limit: query.limit,
  });
}

export function peekAsanaIssueDetails(id: string): AsanaIssueDetails | null {
  return detailsById.get(id) ?? null;
}

export async function asanaIssueDetails(id: string): Promise<AsanaIssueDetails> {
  const generation = cacheGeneration;
  const details = await invoke<AsanaIssueDetails>("asana_issue_details", {
    id,
  });
  if (generation === cacheGeneration) detailsById.set(id, details);
  return details;
}

export function peekAsanaIssueThread(id: string): AsanaIssueThread | null {
  return threadById.get(id) ?? null;
}

export async function asanaIssueThread(
  id: string,
  options?: { force?: boolean },
): Promise<AsanaIssueThread> {
  if (options?.force) {
    threadById.delete(id);
    threadInflight.delete(id);
  }
  const pending = threadInflight.get(id);
  if (pending) return pending;
  const generation = cacheGeneration;
  const promise = invoke<AsanaIssueThread>("asana_issue_thread", { id })
    .then((thread) => {
      if (
        generation === cacheGeneration &&
        threadInflight.get(id) === promise
      ) {
        threadById.set(id, thread);
      }
      return thread;
    })
    .finally(() => {
      if (threadInflight.get(id) === promise) threadInflight.delete(id);
    });
  threadInflight.set(id, promise);
  return promise;
}

export async function asanaIssueComment(
  id: string,
  body: string,
): Promise<string> {
  const storyId = await invoke<string>("asana_issue_comment", {
    id,
    body: body.trim(),
  });
  threadById.delete(id);
  threadInflight.delete(id);
  recordInboxSelfActivity({ provider: "asana", kind: "asana", id });
  return storyId;
}

export function loadHiddenAsanaProjectIds(): string[] {
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

export function saveHiddenAsanaProjectIds(ids: string[]) {
  try {
    localStorage.setItem(PROJECT_IDS_KEY, JSON.stringify(ids));
  } catch {
    // private mode / quota
  }
  notifyAsanaChange();
}

export function loadAsanaProjectLinks(): AsanaProjectLinks {
  try {
    const raw = localStorage.getItem(PROJECT_LINKS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {};
    }
    const links: AsanaProjectLinks = {};
    for (const [id, path] of Object.entries(parsed)) {
      if (id && typeof path === "string" && path.trim()) {
        links[id] = normalizeProjectPath(path);
      }
    }
    return links;
  } catch {
    return {};
  }
}

export function saveAsanaProjectLinks(links: AsanaProjectLinks) {
  try {
    localStorage.setItem(PROJECT_LINKS_KEY, JSON.stringify(links));
  } catch {
    // private mode / quota
  }
  notifyAsanaChange();
}

/** `path` empty removes the link. */
export function linkAsanaProject(
  links: AsanaProjectLinks,
  projectId: string,
  path: string,
): AsanaProjectLinks {
  const next = { ...links };
  if (path.trim()) next[projectId] = normalizeProjectPath(path);
  else delete next[projectId];
  return next;
}

export function asanaProjectIdsLinkedTo(
  links: AsanaProjectLinks,
  path: string,
): string[] {
  if (!path.trim()) return [];
  return Object.entries(links)
    .filter(([, linked]) => sameProjectPath(linked, path))
    .map(([id]) => id);
}

/** The task's first Asana project that is linked to a local project. */
export function linkedAsanaProject(
  projects: readonly AsanaProjectRef[],
  links: AsanaProjectLinks,
): { project: AsanaProjectRef; path: string } | null {
  for (const project of projects) {
    const path = links[project.id];
    if (path) return { project, path };
  }
  return null;
}

export function notifyAsanaChange() {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new Event(ASANA_CHANGE_EVENT));
}
