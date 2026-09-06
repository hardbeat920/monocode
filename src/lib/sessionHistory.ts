import { fuzzyMatch } from "./fuzzy";
import { projectName } from "./paths";
import { sameProjectPath } from "./recents";
import { sessionDisplayTitle, sessionNeedsInput, type Session } from "./session";
import { shouldPersistSession, type SessionSummary } from "./sessionStore";

export type SessionGitHint = {
  repo?: string;
  branch?: string;
};

export function compareSessionSummaries(
  a: SessionSummary,
  b: SessionSummary,
): number {
  const pin = Number(!!b.pinned) - Number(!!a.pinned);
  if (pin !== 0) return pin;
  return b.updatedAt - a.updatedAt || a.id.localeCompare(b.id);
}

export function mergeHistorySummary(
  current: SessionSummary[],
  summary: SessionSummary,
): SessionSummary[] {
  const previous = current.find((entry) => entry.id === summary.id);
  const next = {
    ...summary,
    archived: summary.archived ?? previous?.archived,
    pinned: summary.pinned ?? previous?.pinned,
  };
  return [next, ...current.filter((entry) => entry.id !== summary.id)].sort(
    compareSessionSummaries,
  );
}

/**
 * Swap in one project's freshly fetched rows while leaving every other
 * project's cached rows alone. `history` is keyed only by the `cwd` on each
 * row, so holding several projects at once costs nothing and lets a revisit
 * paint from cache instead of from an empty list.
 */
export function replaceProjectHistory(
  current: SessionSummary[],
  cwd: string,
  rows: SessionSummary[],
): SessionSummary[] {
  const others = current.filter((entry) => !sameProjectPath(entry.cwd, cwd));
  return [...others, ...rows];
}

/**
 * `mergeHistorySummary`, but scoped so persisting a session cannot drop the
 * other projects the cache is holding. A session that changed project is
 * removed from its old one so the id cannot appear twice.
 */
export function mergeProjectHistorySummary(
  current: SessionSummary[],
  summary: SessionSummary,
): SessionSummary[] {
  const mine: SessionSummary[] = [];
  const others: SessionSummary[] = [];
  for (const entry of current) {
    if (sameProjectPath(entry.cwd, summary.cwd)) mine.push(entry);
    else if (entry.id !== summary.id) others.push(entry);
  }
  return [...others, ...mergeHistorySummary(mine, summary)];
}

export function filterSessionsByArchive(
  rows: SessionSummary[],
  showArchived: boolean,
): SessionSummary[] {
  return rows.filter((row) => !!row.archived === showArchived);
}

export function filterSessionsByQuery(
  rows: SessionSummary[],
  query: string,
): SessionSummary[] {
  const needle = query.trim();
  if (!needle) return rows;
  return rows.filter((row) => sessionSearchHit(row, needle));
}

function sessionSearchHit(row: SessionSummary, query: string): boolean {
  const title = sessionDisplayTitle(row.title, row.harness);
  const git = [row.repo, row.branch].filter(Boolean).join("/");
  const fields = [title, row.title, row.model, row.harness, git];
  return fields.some((field) => field && fuzzyMatch(query, field) != null);
}

export function summaryFromSession(
  session: Session,
  git?: SessionGitHint,
): SessionSummary {
  return {
    id: session.id,
    cwd: session.cwd,
    harness: session.harness,
    model: session.model,
    runtimeMode: session.runtimeMode,
    title: session.title,
    providerSessionId: session.providerSessionId,
    ...(git?.branch ? { branch: git.branch } : {}),
    ...(git?.repo ? { repo: git.repo } : {}),
    createdAt: 0,
    updatedAt: Date.now(),
  };
}

/** Prefer the project's persisted origin name, then the overlay / folder name. */
export function projectGitHint(
  rows: SessionSummary[],
  overlay?: SessionGitHint,
): SessionGitHint {
  const repo = rows.find((row) => row.repo)?.repo ?? overlay?.repo;
  const branch = overlay?.branch ?? rows.find((row) => row.branch)?.branch;
  return {
    ...(repo ? { repo } : {}),
    ...(branch ? { branch } : {}),
  };
}

function gitOverlayForCwd(cwd: string, git?: SessionGitHint): SessionGitHint {
  if (git?.repo) return git;
  if (!cwd || cwd === "~") return git ?? {};
  const name = projectName(cwd);
  if (!name || name === "~") return git ?? {};
  return { ...git, repo: name };
}

export function historyWithLiveSessions(
  history: SessionSummary[],
  sessions: Session[],
  cwd: string,
  git?: SessionGitHint,
): SessionSummary[] {
  const inboxIds = new Set(sessions.filter(session => session.inboxAsk).map(session => session.id));
  let rows = history.filter((entry) => !inboxIds.has(entry.id) && sameProjectPath(entry.cwd, cwd));
  const hint = projectGitHint(rows, gitOverlayForCwd(cwd, git));
  for (const session of sessions) {
    if (session.inboxAsk) continue;
    if (!sameProjectPath(session.cwd, cwd)) continue;
    const live = session.busy || sessionNeedsInput(session);
    if (!shouldPersistSession(session) && !live) continue;
    if (rows.some((row) => row.id === session.id)) continue;
    const sessionHint: SessionGitHint = {
      ...hint,
      ...(session.branch ? { branch: session.branch } : {}),
    };
    rows = mergeHistorySummary(rows, summaryFromSession(session, sessionHint));
  }
  return [...rows].sort(compareSessionSummaries);
}
