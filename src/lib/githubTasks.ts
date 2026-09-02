import { invoke } from "@tauri-apps/api/core";
import {
  linearConnected,
  linearTeamIdsForFetch,
  listLinearIssues,
  listLinearTeams,
  loadHiddenLinearTeamIds,
  type LinearIssue,
} from "./linear";
import {
  collectRailProjects,
  normalizeProjectPath,
  sameProjectPath,
  type RecentProject,
} from "./recents";

export type GithubTaskKind = "issue" | "pr";
export type InboxKind = GithubTaskKind | "linear";

export type GithubLabel = {
  name: string;
  color: string;
};

export type GithubAssignee = {
  login: string;
  avatarUrl?: string;
};

export type GithubWorkItem = {
  kind: GithubTaskKind;
  number: number;
  title: string;
  url: string;
  state: string;
  updatedAt: string;
  labels: GithubLabel[];
  assignees: GithubAssignee[];
  draft: boolean;
  repo: string;
};

export type InboxProvider = "github" | "linear";

export type InboxItem = Omit<GithubWorkItem, "kind"> & {
  kind: InboxKind;
  projectPath: string;
  provider: InboxProvider;
  id?: string;
  identifier?: string;
  teamId?: string;
  teamName?: string;
  stateType?: string;
};

export type GithubWorkItemDetails = {
  body: string;
  author: string;
  authorAvatarUrl?: string;
  baseRefName?: string;
  headRefName?: string;
  reviewDecision?: string;
};

export type GithubWorkItemComment = {
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
  replies: GithubWorkItemComment[];
};

export type GithubWorkItemThread = {
  comments: GithubWorkItemComment[];
  truncated: boolean;
  reviewDecision: string;
  baseRefName: string;
  headRefName: string;
};

export type GithubPrFile = {
  path: string;
  additions: number;
  deletions: number;
};

export type GithubPrDiff = {
  additions: number;
  deletions: number;
  files: GithubPrFile[];
  patch: string;
  truncated: boolean;
};

export type GithubWorkItemQuery = {
  kind: GithubTaskKind;
  assignedToMe: boolean;
  state: "open" | "all";
  search: string;
};

export type InboxQuery = Omit<GithubWorkItemQuery, "kind"> & {
  linearHiddenTeamIds?: string[];
};

export type InboxProviderErrors = Partial<Record<InboxProvider, string>>;

export type InboxListResult = {
  items: InboxItem[];
  errors: InboxProviderErrors;
};

const INBOX_CACHE_FRESH_MS = 30_000;

/** Closed history competes for the same slots, so an unfiltered fetch needs the wider page. */
const INBOX_ALL_LIMIT = 100;

type InboxListCache = InboxListResult & {
  key: string;
  fetchedAt: number;
};

let inboxListCache: InboxListCache | null = null;
const inboxListInflight = new Map<string, Promise<InboxListResult>>();
const repoByPath = new Map<string, string>();
const detailsByKey = new Map<string, GithubWorkItemDetails>();
const threadByKey = new Map<string, GithubWorkItemThread>();
const threadInflight = new Map<string, Promise<GithubWorkItemThread>>();
const prDiffByKey = new Map<string, GithubPrDiff>();
const prDiffInflight = new Map<string, Promise<GithubPrDiff>>();

export function clearInboxCache() {
  inboxListCache = null;
  inboxListInflight.clear();
  repoByPath.clear();
  detailsByKey.clear();
  threadByKey.clear();
  threadInflight.clear();
  prDiffByKey.clear();
  prDiffInflight.clear();
}

export function inboxListCacheKey(
  projects: readonly { path: string }[],
  query: InboxQuery,
): string {
  const paths = uniqueInboxProjects(projects)
    .map((project) => normalizeProjectPath(project.path))
    .sort()
    .join("|");
  const teams = [...(query.linearHiddenTeamIds ?? [])].sort().join(",");
  return `${query.assignedToMe ? 1 : 0}:${query.state}:${paths}:${teams}`;
}

export function peekInboxList(
  projects: readonly { path: string }[],
  query: InboxQuery,
): InboxListResult | null {
  const key = inboxListCacheKey(projects, query);
  if (inboxListCache?.key !== key) return null;
  return { items: inboxListCache.items, errors: inboxListCache.errors };
}

export function peekInboxItems(
  projects: readonly { path: string }[],
  query: InboxQuery,
): InboxItem[] | null {
  return peekInboxList(projects, query)?.items ?? null;
}

export function inboxListIsFresh(
  projects: readonly { path: string }[],
  query: InboxQuery,
  now = Date.now(),
): boolean {
  const key = inboxListCacheKey(projects, query);
  return (
    inboxListCache?.key === key &&
    now - inboxListCache.fetchedAt < INBOX_CACHE_FRESH_MS
  );
}

export async function githubRepo(cwd: string): Promise<string> {
  const key = normalizeProjectPath(cwd);
  const cached = repoByPath.get(key);
  if (cached !== undefined) return cached;
  const repo = await invoke<string>("git_github_repo", { cwd });
  repoByPath.set(key, repo);
  return repo;
}

export function listGithubWorkItems(
  cwd: string,
  query: GithubWorkItemQuery,
): Promise<GithubWorkItem[]> {
  return invoke<GithubWorkItem[]>("git_github_work_items", {
    cwd,
    kind: query.kind,
    assignedToMe: query.assignedToMe,
    state: query.state,
    search: query.search.trim(),
    limit: query.state === "all" ? INBOX_ALL_LIMIT : undefined,
  });
}

export function formatGithubQuery(query: GithubWorkItemQuery): string {
  const parts: string[] = [];
  if (query.assignedToMe) parts.push("assignee:@me");
  parts.push(query.kind === "pr" ? "is:pr" : "is:issue");
  if (query.state === "open") parts.push("is:open");
  const text = query.search.trim();
  if (text) parts.push(text);
  return parts.join(" ");
}

export function githubAvatarUrl(login: string, size = 64): string {
  const name = login.trim();
  if (!name) return "";
  return `https://avatars.githubusercontent.com/${encodeURIComponent(name)}?s=${size}`;
}

export function inboxPersonAvatarUrl(
  provider: InboxProvider,
  login: string,
  avatarUrl?: string,
): string {
  const explicit = avatarUrl?.trim() ?? "";
  if (explicit) return explicit;
  if (provider === "github") return githubAvatarUrl(login);
  return "";
}

export function formatRelativeTime(
  iso: string,
  now = Date.now(),
  locale?: string,
): string {
  const then = Date.parse(iso);
  if (!Number.isFinite(then)) return "";
  const delta = Math.round((then - now) / 1000);
  const abs = Math.abs(delta);
  const divisions: [number, Intl.RelativeTimeFormatUnit][] = [
    [60, "second"],
    [60, "minute"],
    [24, "hour"],
    [7, "day"],
    [4.34524, "week"],
    [12, "month"],
    [Number.POSITIVE_INFINITY, "year"],
  ];
  let value = delta;
  let unit: Intl.RelativeTimeFormatUnit = "second";
  let amount = abs;
  for (const [step, next] of divisions) {
    unit = next;
    if (amount < step) break;
    value = Math.round(value / step);
    amount = Math.abs(value);
  }
  try {
    return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(
      value,
      unit,
    );
  } catch {
    return "";
  }
}

export function detailsCacheKey(
  cwd: string,
  kind: GithubTaskKind,
  number: number,
): string {
  return `${normalizeProjectPath(cwd)}:${kind}:${number}`;
}

export function peekGithubWorkItemDetails(
  cwd: string,
  kind: GithubTaskKind,
  number: number,
): GithubWorkItemDetails | null {
  return detailsByKey.get(detailsCacheKey(cwd, kind, number)) ?? null;
}

export async function githubWorkItemDetails(
  cwd: string,
  kind: GithubTaskKind,
  number: number,
): Promise<GithubWorkItemDetails> {
  const details = await invoke<GithubWorkItemDetails>(
    "git_github_work_item_details",
    { cwd, kind, number },
  );
  detailsByKey.set(detailsCacheKey(cwd, kind, number), details);
  return details;
}

export function peekGithubWorkItemThread(
  cwd: string,
  kind: GithubTaskKind,
  number: number,
): GithubWorkItemThread | null {
  return threadByKey.get(detailsCacheKey(cwd, kind, number)) ?? null;
}

export async function githubWorkItemThread(
  cwd: string,
  kind: GithubTaskKind,
  number: number,
  options?: { force?: boolean },
): Promise<GithubWorkItemThread> {
  const key = detailsCacheKey(cwd, kind, number);
  if (options?.force) {
    threadByKey.delete(key);
    threadInflight.delete(key);
  }
  const pending = threadInflight.get(key);
  if (pending) return pending;
  const promise = invoke<GithubWorkItemThread>("git_github_work_item_thread", {
    cwd,
    kind,
    number,
  })
    .then((thread) => {
      threadByKey.set(key, thread);
      return thread;
    })
    .finally(() => {
      if (threadInflight.get(key) === promise) threadInflight.delete(key);
    });
  threadInflight.set(key, promise);
  return promise;
}

export async function githubWorkItemComment(
  cwd: string,
  kind: GithubTaskKind,
  number: number,
  body: string,
  options?: { inReplyTo?: string },
): Promise<string> {
  const url = await invoke<string>("git_github_work_item_comment", {
    cwd,
    kind,
    number,
    body: body.trim(),
    inReplyTo: options?.inReplyTo?.trim() ?? "",
  });
  const key = detailsCacheKey(cwd, kind, number);
  threadByKey.delete(key);
  threadInflight.delete(key);
  return url;
}

export function githubReviewDecisionLabel(decision: string): string {
  switch (decision.trim().toUpperCase()) {
    case "APPROVED":
      return "Approved";
    case "CHANGES_REQUESTED":
      return "Changes requested";
    case "REVIEW_REQUIRED":
      return "Review required";
    default:
      return "";
  }
}

export function githubReviewStateLabel(state: string): string {
  switch (state.trim().toUpperCase()) {
    case "APPROVED":
      return "Approved";
    case "CHANGES_REQUESTED":
      return "Requested changes";
    case "DISMISSED":
      return "Dismissed";
    case "COMMENTED":
      return "Commented";
    default:
      return "";
  }
}

export function prDiffCacheKey(cwd: string, number: number): string {
  return `${normalizeProjectPath(cwd)}:pr:${number}`;
}

export function peekGithubPrDiff(
  cwd: string,
  number: number,
): GithubPrDiff | null {
  return prDiffByKey.get(prDiffCacheKey(cwd, number)) ?? null;
}

export async function githubPrDiff(
  cwd: string,
  number: number,
): Promise<GithubPrDiff> {
  const key = prDiffCacheKey(cwd, number);
  const pending = prDiffInflight.get(key);
  if (pending) return pending;
  const promise = invoke<GithubPrDiff>("git_github_pr_diff", { cwd, number })
    .then((diff) => {
      prDiffByKey.set(key, diff);
      return diff;
    })
    .finally(() => {
      if (prDiffInflight.get(key) === promise) prDiffInflight.delete(key);
    });
  prDiffInflight.set(key, promise);
  return promise;
}

export async function listInboxItems(
  projects: readonly { path: string }[],
  query: InboxQuery,
  options?: { force?: boolean },
): Promise<InboxListResult> {
  const key = inboxListCacheKey(projects, query);
  if (!options?.force && inboxListIsFresh(projects, query)) {
    return peekInboxList(projects, query) ?? { items: [], errors: {} };
  }
  const pending = inboxListInflight.get(key);
  if (pending) return pending;
  const promise = fetchInboxItems(projects, query)
    .then((result) => {
      inboxListCache = { key, ...result, fetchedAt: Date.now() };
      return result;
    })
    .finally(() => {
      if (inboxListInflight.get(key) === promise) inboxListInflight.delete(key);
    });
  inboxListInflight.set(key, promise);
  return promise;
}

async function fetchInboxItems(
  projects: readonly { path: string }[],
  query: InboxQuery,
): Promise<InboxListResult> {
  const unique = uniqueInboxProjects(projects);
  const preferredPaths = unique.map((project) => project.path);
  const resolved = await Promise.all(
    unique.map(async (project) => {
      try {
        return {
          path: project.path,
          repo: (await githubRepo(project.path)).trim(),
        };
      } catch {
        return { path: project.path, repo: "" };
      }
    }),
  );
  const grouped = groupProjectsByRepo(resolved);
  const githubJobs = grouped.flatMap((project) =>
    (["issue", "pr"] as const).map(async (kind) => {
      const items = await listGithubWorkItems(project.path, {
        ...query,
        kind,
      });
      return items.map((item) => ({
        ...item,
        projectPath: project.path,
        provider: "github" as const,
        repo: item.repo || project.repo,
      }));
    }),
  );
  const github = collectInboxResults(
    await Promise.allSettled(githubJobs),
    preferredPaths,
  );
  const errors: InboxProviderErrors = {};
  if (github.error && grouped.length > 0) errors.github = github.error;

  let linearItems: InboxItem[] = [];
  if ((await linearConnected()).connected) {
    try {
      linearItems = await fetchLinearInboxItems(query);
    } catch (error) {
      errors.linear = inboxErrorMessage(error);
    }
  }

  return {
    items: dedupeInboxItems([...github.items, ...linearItems], preferredPaths),
    errors,
  };
}

async function fetchLinearInboxItems(query: InboxQuery): Promise<InboxItem[]> {
  const hiddenIds = query.linearHiddenTeamIds ?? loadHiddenLinearTeamIds();
  let teamIds: string[] | null = null;
  if (hiddenIds.length > 0) {
    teamIds = linearTeamIdsForFetch(await listLinearTeams(), hiddenIds);
    if (teamIds?.length === 0) return [];
  }
  const issues = await listLinearIssues({
    assignedToMe: query.assignedToMe,
    state: query.state,
    teamIds: teamIds ?? [],
    limit: query.state === "all" ? INBOX_ALL_LIMIT : undefined,
  });
  const hidden = new Set(hiddenIds);
  return issues
    .filter((issue) => hidden.size === 0 || !hidden.has(issue.teamId))
    .map(linearIssueToInboxItem);
}

function linearIssueToInboxItem(issue: LinearIssue): InboxItem {
  return {
    provider: "linear",
    kind: "linear",
    id: issue.id,
    identifier: issue.identifier,
    number: issue.number,
    title: issue.title,
    url: issue.url,
    state: issue.state,
    stateType: issue.stateType,
    updatedAt: issue.updatedAt,
    labels: issue.labels,
    assignees: issue.assignees,
    draft: false,
    repo: issue.repo,
    teamId: issue.teamId,
    teamName: issue.teamName,
    projectPath: issue.projectPath || "",
  };
}

export function inboxProjectsForRail(
  recents: RecentProject[],
  cwd: string,
): RecentProject[] {
  const map = collectRailProjects(recents, cwd);
  const current = cwd ? map.get(normalizeProjectPath(cwd)) : undefined;
  const rest = [...map.values()].filter(
    (project) => !current || !sameProjectPath(project.path, current.path),
  );
  return current ? [current, ...rest] : rest;
}

export function uniqueInboxProjects(
  projects: readonly { path: string }[],
): { path: string }[] {
  const seen = new Set<string>();
  const unique: { path: string }[] = [];
  for (const project of projects) {
    const path = normalizeProjectPath(project.path);
    if (!path || seen.has(path)) continue;
    seen.add(path);
    unique.push({ path });
  }
  return unique;
}

export function groupProjectsByRepo(
  resolved: readonly { path: string; repo: string }[],
): { path: string; repo: string }[] {
  const seen = new Set<string>();
  const grouped: { path: string; repo: string }[] = [];
  for (const project of resolved) {
    const repo = project.repo.trim().toLowerCase();
    const key = repo || `path:${normalizeProjectPath(project.path)}`;
    if (seen.has(key)) continue;
    seen.add(key);
    grouped.push({
      path: project.path,
      repo: project.repo.trim(),
    });
  }
  return grouped;
}

export function collectInboxResults(
  settled: PromiseSettledResult<InboxItem[]>[],
  preferredPaths: readonly string[] = [],
): { items: InboxItem[]; error?: string } {
  const batches: InboxItem[][] = [];
  const errors: unknown[] = [];
  for (const result of settled) {
    if (result.status === "fulfilled") batches.push(result.value);
    else errors.push(result.reason);
  }
  if (batches.length === 0 && errors.length > 0) {
    return { items: [], error: inboxErrorMessage(errors[0]) };
  }
  return { items: dedupeInboxItems(batches.flat(), preferredPaths) };
}

function inboxErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function inboxIdentityKey(item: {
  provider?: InboxProvider;
  kind: InboxKind;
  number: number;
  repo: string;
  url: string;
  identifier?: string;
  id?: string;
}): string {
  if (item.provider === "linear") {
    const identity = item.identifier?.trim() || item.id?.trim();
    if (identity) return identity.toLowerCase();
    return `linear:${item.number}`;
  }
  const repo = item.repo.trim().toLowerCase();
  if (repo) return `${repo}:${item.kind}:${item.number}`;
  const url = item.url.trim().toLowerCase();
  if (url) return url;
  return `${item.kind}:${item.number}`;
}

export function dedupeInboxItems(
  items: readonly InboxItem[],
  preferredPaths: readonly string[] = [],
): InboxItem[] {
  const rank = new Map(
    preferredPaths.map((path, index) => [normalizeProjectPath(path), index]),
  );
  const best = new Map<string, InboxItem>();
  for (const item of items) {
    const key = inboxIdentityKey(item);
    const current = best.get(key);
    if (!current || preferInboxItem(item, current, rank)) best.set(key, item);
  }
  return sortInboxItems([...best.values()]);
}

function preferInboxItem(
  next: InboxItem,
  current: InboxItem,
  rank: Map<string, number>,
): boolean {
  const nextRank =
    rank.get(normalizeProjectPath(next.projectPath)) ??
    Number.POSITIVE_INFINITY;
  const currentRank =
    rank.get(normalizeProjectPath(current.projectPath)) ??
    Number.POSITIVE_INFINITY;
  if (nextRank !== currentRank) return nextRank < currentRank;
  return next.projectPath.localeCompare(current.projectPath) < 0;
}

export function sortInboxItems(items: InboxItem[]): InboxItem[] {
  return [...items].sort((a, b) => {
    const updated =
      (Date.parse(b.updatedAt) || 0) - (Date.parse(a.updatedAt) || 0);
    if (updated !== 0) return updated;
    if (a.projectPath !== b.projectPath) {
      return a.projectPath.localeCompare(b.projectPath);
    }
    if (a.kind !== b.kind) return a.kind.localeCompare(b.kind);
    return b.number - a.number;
  });
}

export function inboxItemKey(item: InboxItem): string {
  return `${item.provider}:${inboxIdentityKey(item)}`;
}

export function githubWorkItemKey(item: GithubWorkItem): string {
  return `${item.repo}:${item.kind}:${item.number}`;
}

export function inboxItemStatus(item: {
  kind: InboxKind;
  state: string;
  draft: boolean;
  stateType?: string;
}): string {
  if (item.kind === "linear") {
    const type = item.stateType?.trim().toLowerCase();
    if (type === "completed" || type === "canceled") return "Closed";
    return "Open";
  }
  if (item.draft) return "Draft";
  if (item.state === "merged") return "Merged";
  if (item.state === "closed") return "Closed";
  return "Open";
}

export function matchesInboxQuery(item: InboxItem, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  const kind =
    item.kind === "pr"
      ? "pull request pr"
      : item.kind === "linear"
        ? "linear issue"
        : "issue";
  const haystack = [
    item.title,
    item.repo,
    item.projectPath,
    item.identifier,
    item.teamName,
    kind,
    `#${item.number}`,
    String(item.number),
    ...item.labels.map((label) => label.name),
    ...item.assignees.map((person) => person.login),
  ]
    .join(" ")
    .toLowerCase();
  return haystack.includes(needle);
}

export function filterInboxItems(
  items: readonly InboxItem[],
  query: string,
): InboxItem[] {
  return items.filter((item) => matchesInboxQuery(item, query));
}

export function inboxItemRef(item: {
  provider?: InboxProvider;
  number: number;
  identifier?: string;
}): string {
  if (item.provider === "linear") {
    return item.identifier?.trim() || `#${item.number}`;
  }
  return `#${item.number}`;
}

export function inboxStartDraft(item: InboxItem, body?: string): string {
  if (item.provider === "linear") {
    const id = item.identifier?.trim() || `Linear #${item.number}`;
    const title = item.title.trim() || id;
    const lines = ["Work on this Linear issue:", "", `${id} ${title}`];
    const url = item.url.trim();
    if (url) lines.push(url);
    const description = body?.trim();
    if (description) {
      lines.push("", description);
    }
    return `${lines.join("\n")}\n`;
  }
  const kind = item.kind === "pr" ? "pull request" : "issue";
  const title = item.title.trim() || `GitHub ${kind} #${item.number}`;
  const lines = [
    `Work on this GitHub ${kind}:`,
    "",
    `#${item.number} ${title}`,
  ];
  const url = item.url.trim();
  if (url) lines.push(url);
  return `${lines.join("\n")}\n`;
}

/** Compact chip shown above the composer when starting from Inbox. */
export type InboxComposerCard = {
  provider: InboxProvider;
  kind: InboxKind;
  identifier: string;
  title: string;
  url: string;
  source: string;
  labels: GithubLabel[];
  prompt: string;
};

export function inboxComposerCard(
  item: InboxItem,
  body?: string,
): InboxComposerCard {
  const linear = item.provider === "linear";
  return {
    provider: item.provider,
    kind: item.kind,
    identifier: inboxItemRef(item),
    title: item.title.trim() || inboxItemRef(item),
    url: item.url.trim(),
    source: linear ? item.teamName || item.repo : item.repo,
    labels: item.labels.slice(0, 2),
    prompt: inboxStartDraft(item, body).trimEnd(),
  };
}

export function composeInboxMessage(
  card: InboxComposerCard | undefined,
  text: string,
): string {
  const prompt = card?.prompt.trim() ?? "";
  const note = text.trim();
  if (!prompt) return note;
  if (!note) return prompt;
  return `${prompt}\n\n${note}`;
}
