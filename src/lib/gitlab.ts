import { invoke } from "@tauri-apps/api/core";
import { normalizeProjectPath } from "./recents";

export type GitlabKind = "issue" | "pr";

export type GitlabStatus = {
  connected: boolean;
  url: string;
};

export type GitlabWorkItem = {
  kind: GitlabKind;
  number: number;
  title: string;
  url: string;
  state: string;
  updatedAt: string;
  labels: { name: string; color: string }[];
  assignees: { login: string; avatarUrl?: string }[];
  draft: boolean;
  repo: string;
  attentionReason: string;
};

export type GitlabWorkItemDetails = {
  body: string;
  author: string;
  authorAvatarUrl?: string;
  baseRefName?: string;
  headRefName?: string;
  reviewDecision?: string;
};

export type GitlabWorkItemComment = {
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
  replies: GitlabWorkItemComment[];
};

export type GitlabWorkItemThread = {
  comments: GitlabWorkItemComment[];
  truncated: boolean;
  reviewDecision: string;
  baseRefName: string;
  headRefName: string;
};

export type GitlabMrDiff = {
  additions: number;
  deletions: number;
  files: { path: string; additions: number; deletions: number }[];
  patch: string;
  truncated: boolean;
};

export const GITLAB_CHANGE_EVENT = "monocode:gitlab-change";

const repoByPath = new Map<string, string>();
const detailsByKey = new Map<string, GitlabWorkItemDetails>();
const threadByKey = new Map<string, GitlabWorkItemThread>();
const threadInflight = new Map<string, Promise<GitlabWorkItemThread>>();
const diffByKey = new Map<string, GitlabMrDiff>();
const diffInflight = new Map<string, Promise<GitlabMrDiff>>();

function itemKey(repo: string, kind: GitlabKind, number: number): string {
  return `${repo.trim().toLowerCase()}:${kind}:${number}`;
}

export function clearGitlabCache() {
  repoByPath.clear();
  detailsByKey.clear();
  threadByKey.clear();
  threadInflight.clear();
  diffByKey.clear();
  diffInflight.clear();
}

export function gitlabConnected(): Promise<GitlabStatus> {
  return invoke<GitlabStatus>("gitlab_status");
}

export async function saveGitlabConfig(
  url: string,
  token: string,
): Promise<GitlabStatus> {
  const status = await invoke<GitlabStatus>("gitlab_set_config", {
    url: url.trim(),
    token: token.trim(),
  });
  clearGitlabCache();
  notifyGitlabChange();
  return status;
}

export async function disconnectGitlab(url: string): Promise<GitlabStatus> {
  const status = await invoke<GitlabStatus>("gitlab_set_config", {
    url: url.trim(),
    token: "",
  });
  clearGitlabCache();
  notifyGitlabChange();
  return status;
}

export async function gitlabRepo(cwd: string): Promise<string> {
  const key = normalizeProjectPath(cwd);
  const cached = repoByPath.get(key);
  if (cached !== undefined) return cached;
  const repo = await invoke<string>("gitlab_repo", { cwd });
  repoByPath.set(key, repo);
  return repo;
}

export function listGitlabWorkItems(
  cwd: string,
  query: {
    kind: GitlabKind;
    assignedToMe: boolean;
    state: "open" | "all";
    limit?: number;
  },
): Promise<GitlabWorkItem[]> {
  return invoke<GitlabWorkItem[]>("gitlab_list_work_items", {
    cwd,
    kind: query.kind,
    assignedToMe: query.assignedToMe,
    state: query.state,
    limit: query.limit,
  });
}

export function listGitlabTodos(query: {
  kind: GitlabKind;
  limit?: number;
}): Promise<GitlabWorkItem[]> {
  return invoke<GitlabWorkItem[]>("gitlab_list_todos", {
    kind: query.kind,
    limit: query.limit,
  });
}

export function peekGitlabWorkItemDetails(
  repo: string,
  kind: GitlabKind,
  number: number,
): GitlabWorkItemDetails | null {
  return detailsByKey.get(itemKey(repo, kind, number)) ?? null;
}

export async function gitlabWorkItemDetails(
  repo: string,
  kind: GitlabKind,
  number: number,
): Promise<GitlabWorkItemDetails> {
  const details = await invoke<GitlabWorkItemDetails>(
    "gitlab_work_item_details",
    { repo, kind, number },
  );
  detailsByKey.set(itemKey(repo, kind, number), details);
  return details;
}

export function peekGitlabWorkItemThread(
  repo: string,
  kind: GitlabKind,
  number: number,
): GitlabWorkItemThread | null {
  return threadByKey.get(itemKey(repo, kind, number)) ?? null;
}

export async function gitlabWorkItemThread(
  repo: string,
  kind: GitlabKind,
  number: number,
  options?: { force?: boolean },
): Promise<GitlabWorkItemThread> {
  const key = itemKey(repo, kind, number);
  if (options?.force) {
    threadByKey.delete(key);
    threadInflight.delete(key);
  }
  const cached = threadInflight.get(key);
  if (cached) return cached;
  const pending = invoke<GitlabWorkItemThread>("gitlab_work_item_thread", {
    repo,
    kind,
    number,
  })
    .then((thread) => {
      threadByKey.set(key, thread);
      return thread;
    })
    .finally(() => {
      if (threadInflight.get(key) === pending) threadInflight.delete(key);
    });
  threadInflight.set(key, pending);
  return pending;
}

export async function gitlabWorkItemComment(
  repo: string,
  kind: GitlabKind,
  number: number,
  body: string,
): Promise<string> {
  const url = await invoke<string>("gitlab_work_item_comment", {
    repo,
    kind,
    number,
    body: body.trim(),
  });
  const key = itemKey(repo, kind, number);
  threadByKey.delete(key);
  threadInflight.delete(key);
  return url;
}

export function peekGitlabMrDiff(
  repo: string,
  number: number,
): GitlabMrDiff | null {
  return diffByKey.get(itemKey(repo, "pr", number)) ?? null;
}

export async function gitlabMrDiff(
  repo: string,
  number: number,
): Promise<GitlabMrDiff> {
  const key = itemKey(repo, "pr", number);
  const cached = diffInflight.get(key);
  if (cached) return cached;
  const pending = invoke<GitlabMrDiff>("gitlab_mr_diff", { repo, number })
    .then((diff) => {
      diffByKey.set(key, diff);
      return diff;
    })
    .finally(() => {
      if (diffInflight.get(key) === pending) diffInflight.delete(key);
    });
  diffInflight.set(key, pending);
  return pending;
}

export function notifyGitlabChange() {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new Event(GITLAB_CHANGE_EVENT));
}
