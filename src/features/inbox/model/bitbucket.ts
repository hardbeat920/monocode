import { invoke } from "@tauri-apps/api/core";
import { recordInboxSelfActivity } from "./inboxSelfActivity";
import { normalizeProjectPath } from "../../projects/model/recents";

export type BitbucketKind = "issue" | "pr";

export type BitbucketStatus = {
  connected: boolean;
  email: string;
};

export type BitbucketWorkItem = {
  kind: BitbucketKind;
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

export type BitbucketWorkItemDetails = {
  body: string;
  author: string;
  authorAvatarUrl?: string;
  baseRefName?: string;
  headRefName?: string;
  reviewDecision?: string;
};

export type BitbucketWorkItemComment = {
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
  replies: BitbucketWorkItemComment[];
};

export type BitbucketWorkItemThread = {
  comments: BitbucketWorkItemComment[];
  truncated: boolean;
  reviewDecision: string;
  baseRefName: string;
  headRefName: string;
};

export type BitbucketPrDiff = {
  additions: number;
  deletions: number;
  files: { path: string; additions: number; deletions: number }[];
  patch: string;
  truncated: boolean;
};

export const BITBUCKET_CHANGE_EVENT = "monocode:bitbucket-change";

const repoByPath = new Map<string, string>();
const detailsByKey = new Map<string, BitbucketWorkItemDetails>();
const threadByKey = new Map<string, BitbucketWorkItemThread>();
const threadInflight = new Map<string, Promise<BitbucketWorkItemThread>>();
const diffByKey = new Map<string, BitbucketPrDiff>();
const diffInflight = new Map<string, Promise<BitbucketPrDiff>>();

function itemKey(repo: string, kind: BitbucketKind, number: number): string {
  return `${repo.trim().toLowerCase()}:${kind}:${number}`;
}

export function clearBitbucketCache() {
  repoByPath.clear();
  detailsByKey.clear();
  threadByKey.clear();
  threadInflight.clear();
  diffByKey.clear();
  diffInflight.clear();
}

export function bitbucketConnected(): Promise<BitbucketStatus> {
  return invoke<BitbucketStatus>("bitbucket_status");
}

export async function saveBitbucketConfig(
  email: string,
  token: string,
): Promise<BitbucketStatus> {
  const status = await invoke<BitbucketStatus>("bitbucket_set_config", {
    email: email.trim(),
    token: token.trim(),
  });
  clearBitbucketCache();
  notifyBitbucketChange();
  return status;
}

export async function disconnectBitbucket(
  email: string,
): Promise<BitbucketStatus> {
  const status = await invoke<BitbucketStatus>("bitbucket_set_config", {
    email: email.trim(),
    token: "",
  });
  clearBitbucketCache();
  notifyBitbucketChange();
  return status;
}

export async function bitbucketRepo(cwd: string): Promise<string> {
  const key = normalizeProjectPath(cwd);
  const cached = repoByPath.get(key);
  if (cached !== undefined) return cached;
  const repo = await invoke<string>("bitbucket_repo", { cwd });
  repoByPath.set(key, repo);
  return repo;
}

export function listBitbucketWorkItems(
  cwd: string,
  query: {
    kind: BitbucketKind;
    assignedToMe: boolean;
    state: "open" | "all";
    limit?: number;
  },
): Promise<BitbucketWorkItem[]> {
  return invoke<BitbucketWorkItem[]>("bitbucket_list_work_items", {
    cwd,
    kind: query.kind,
    assignedToMe: query.assignedToMe,
    state: query.state,
    limit: query.limit,
  });
}

export function peekBitbucketWorkItemDetails(
  repo: string,
  kind: BitbucketKind,
  number: number,
): BitbucketWorkItemDetails | null {
  return detailsByKey.get(itemKey(repo, kind, number)) ?? null;
}

export async function bitbucketWorkItemDetails(
  repo: string,
  kind: BitbucketKind,
  number: number,
): Promise<BitbucketWorkItemDetails> {
  const details = await invoke<BitbucketWorkItemDetails>(
    "bitbucket_work_item_details",
    { repo, kind, number },
  );
  detailsByKey.set(itemKey(repo, kind, number), details);
  return details;
}

export function peekBitbucketWorkItemThread(
  repo: string,
  kind: BitbucketKind,
  number: number,
): BitbucketWorkItemThread | null {
  return threadByKey.get(itemKey(repo, kind, number)) ?? null;
}

export async function bitbucketWorkItemThread(
  repo: string,
  kind: BitbucketKind,
  number: number,
  options?: { force?: boolean },
): Promise<BitbucketWorkItemThread> {
  const key = itemKey(repo, kind, number);
  if (options?.force) {
    threadByKey.delete(key);
    threadInflight.delete(key);
  }
  const cached = threadInflight.get(key);
  if (cached) return cached;
  const pending = invoke<BitbucketWorkItemThread>(
    "bitbucket_work_item_thread",
    {
      repo,
      kind,
      number,
    },
  )
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

export async function bitbucketWorkItemComment(
  repo: string,
  kind: BitbucketKind,
  number: number,
  body: string,
): Promise<string> {
  const url = await invoke<string>("bitbucket_work_item_comment", {
    repo,
    kind,
    number,
    body: body.trim(),
  });
  const key = itemKey(repo, kind, number);
  threadByKey.delete(key);
  threadInflight.delete(key);
  recordInboxSelfActivity({ provider: "bitbucket", kind, repo, number });
  return url;
}

export function peekBitbucketPrDiff(
  repo: string,
  number: number,
): BitbucketPrDiff | null {
  return diffByKey.get(itemKey(repo, "pr", number)) ?? null;
}

export async function bitbucketPrDiff(
  repo: string,
  number: number,
): Promise<BitbucketPrDiff> {
  const key = itemKey(repo, "pr", number);
  const cached = diffInflight.get(key);
  if (cached) return cached;
  const pending = invoke<BitbucketPrDiff>("bitbucket_pr_diff", { repo, number })
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

export function notifyBitbucketChange() {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new Event(BITBUCKET_CHANGE_EVENT));
}
