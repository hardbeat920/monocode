import { fuzzyMatch } from "./fuzzy";
import { projectName } from "./paths";
import { sameProjectPath, type RecentProject } from "./recents";
import {
  HARNESSES,
  sessionDisplayTitle,
  type Block,
  type HarnessId,
  type Session,
} from "./session";
import type {
  SessionSearchHit,
  SessionSummary,
} from "./sessionStore";
import type { RankedFile } from "./fileIndex";
import type { ProjectSearchMatch } from "./search";

export type SearchScope = "all" | "conversations" | "files" | "projects";

export type ConversationHit = {
  id: string;
  kind: "conversation";
  sessionId: string;
  cwd: string;
  harness: HarnessId;
  title: string;
  updatedAt: number;
  score: number;
  positions: number[];
};

export type MessageHit = {
  id: string;
  kind: "message";
  sessionId: string;
  cwd: string;
  harness: HarnessId;
  title: string;
  updatedAt: number;
  blockId: string;
  role: string;
  preview: string;
  score: number;
};

export type FileHit = {
  id: string;
  kind: "file";
  path: string;
  relative: string;
  name: string;
  score: number;
  positions: number[];
};

export type ContentHit = {
  id: string;
  kind: "content";
  path: string;
  relative: string;
  name: string;
  line: number;
  column: number;
  preview: string;
};

export type ProjectHit = {
  id: string;
  kind: "project";
  path: string;
  name: string;
  score: number;
  positions: number[];
};

export type AppSearchHit =
  | ConversationHit
  | MessageHit
  | FileHit
  | ContentHit
  | ProjectHit;

export type GroupedHits = {
  conversations: ConversationHit[];
  messages: MessageHit[];
  files: FileHit[];
  content: ContentHit[];
  projects: ProjectHit[];
};

const ALL_LIMITS: Record<keyof GroupedHits, number> = {
  conversations: 8,
  messages: 8,
  files: 10,
  content: 12,
  projects: 6,
};

const SCOPE_LIMITS: Record<SearchScope, Record<keyof GroupedHits, number>> = {
  all: ALL_LIMITS,
  conversations: {
    conversations: 24,
    messages: 24,
    files: 0,
    content: 0,
    projects: 0,
  },
  files: {
    conversations: 0,
    messages: 0,
    files: 40,
    content: 48,
    projects: 0,
  },
  projects: {
    conversations: 0,
    messages: 0,
    files: 0,
    content: 0,
    projects: 24,
  },
};

export function asHarness(value: string): HarnessId {
  return (HARNESSES as string[]).includes(value)
    ? (value as HarnessId)
    : "cursor";
}

export function snippetAround(
  text: string,
  query: string,
  radius = 42,
): string {
  const compact = text.replace(/\s+/g, " ").trim();
  const needle = query.trim().toLowerCase();
  if (!compact) return "";
  if (!needle) {
    return compact.length > radius * 2
      ? `${compact.slice(0, radius * 2)}…`
      : compact;
  }
  const index = compact.toLowerCase().indexOf(needle);
  if (index < 0) {
    return compact.length > radius * 2
      ? `${compact.slice(0, radius * 2)}…`
      : compact;
  }
  const start = Math.max(0, index - radius);
  const end = Math.min(compact.length, index + needle.length + radius);
  let snippet = compact.slice(start, end).trim();
  if (start > 0) snippet = `…${snippet}`;
  if (end < compact.length) snippet = `${snippet}…`;
  return snippet;
}

export function searchConversationTitles(
  rows: Array<{
    id: string;
    cwd: string;
    harness: HarnessId;
    title: string;
    updatedAt: number;
  }>,
  query: string,
): ConversationHit[] {
  const needle = query.trim();
  if (!needle) return [];
  const hits: ConversationHit[] = [];
  for (const row of rows) {
    const title = sessionDisplayTitle(row.title, row.harness);
    const displayHit = fuzzyMatch(needle, title);
    const rawHit = displayHit ? null : fuzzyMatch(needle, row.title);
    const match = displayHit ?? rawHit;
    if (!match) continue;
    hits.push({
      id: `conversation:${row.id}`,
      kind: "conversation",
      sessionId: row.id,
      cwd: row.cwd,
      harness: row.harness,
      title,
      updatedAt: row.updatedAt,
      score: match.score + recencyBonus(row.updatedAt),
      positions: displayHit ? match.positions : [],
    });
  }
  return hits.sort(byScoreThenRecency);
}

export function searchSessionMessages(
  sessions: Array<{
    id: string;
    cwd: string;
    harness: HarnessId;
    title: string;
    updatedAt: number;
    blocks: Block[];
  }>,
  query: string,
): MessageHit[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const hits: MessageHit[] = [];
  for (const session of sessions) {
    const title = sessionDisplayTitle(session.title, session.harness);
    for (const block of session.blocks) {
      if (!isSearchableRole(block.role)) continue;
      const text = blockSearchText(block);
      if (!text.toLowerCase().includes(needle)) continue;
      hits.push({
        id: `message:${session.id}:${block.id}`,
        kind: "message",
        sessionId: session.id,
        cwd: session.cwd,
        harness: session.harness,
        title,
        updatedAt: session.updatedAt,
        blockId: block.id,
        role: block.role,
        preview: snippetAround(text, query),
        score: 20 + recencyBonus(session.updatedAt),
      });
    }
  }
  return hits.sort(byScoreThenRecency);
}

export function searchRecentProjects(
  recents: RecentProject[],
  query: string,
): ProjectHit[] {
  const needle = query.trim();
  if (!needle) return [];
  const hits: ProjectHit[] = [];
  for (const recent of recents) {
    const name = projectName(recent.path);
    const nameHit = fuzzyMatch(needle, name);
    const pathHit = nameHit ? null : fuzzyMatch(needle, recent.path);
    const match = nameHit ?? pathHit;
    if (!match) continue;
    hits.push({
      id: `project:${recent.path}`,
      kind: "project",
      path: recent.path,
      name,
      score: match.score + (nameHit ? 80 : 0),
      positions: nameHit ? match.positions : [],
    });
  }
  return hits.sort((a, b) => b.score - a.score || a.name.localeCompare(b.name));
}

export function hitsFromFileRanks(files: RankedFile[]): FileHit[] {
  return files.map((file) => ({
    id: `file:${file.path}`,
    kind: "file",
    path: file.path,
    relative: file.relative,
    name: file.name,
    score: file.score,
    positions: file.positions,
  }));
}

export function hitsFromContentMatches(
  matches: ProjectSearchMatch[],
): ContentHit[] {
  return matches.map((match) => ({
    id: `content:${match.path}:${match.line}:${match.column}`,
    kind: "content",
    path: match.path,
    relative: match.relative,
    name: match.relative.split("/").pop() ?? match.relative,
    line: match.line,
    column: match.column,
    preview: match.preview.trimEnd(),
  }));
}

export function hitsFromSessionSearch(
  rows: SessionSearchHit[],
): AppSearchHit[] {
  const hits: AppSearchHit[] = [];
  for (const row of rows) {
    const harness = asHarness(row.harness);
    const title = sessionDisplayTitle(row.title, harness);
    if (row.kind === "conversation") {
      hits.push({
        id: `conversation:${row.sessionId}`,
        kind: "conversation",
        sessionId: row.sessionId,
        cwd: row.cwd,
        harness,
        title,
        updatedAt: row.updatedAt,
        score: 10 + recencyBonus(row.updatedAt),
        positions: [],
      });
      continue;
    }
    if (row.kind !== "message" || !row.blockId) continue;
    hits.push({
      id: `message:${row.sessionId}:${row.blockId}`,
      kind: "message",
      sessionId: row.sessionId,
      cwd: row.cwd,
      harness,
      title,
      updatedAt: row.updatedAt,
      blockId: row.blockId,
      role: row.role ?? "assistant",
      preview: row.preview,
      score: 16 + recencyBonus(row.updatedAt),
    });
  }
  return hits;
}

export function conversationRowsFrom(
  history: SessionSummary[],
  sessions: Session[],
): Array<{
  id: string;
  cwd: string;
  harness: HarnessId;
  title: string;
  updatedAt: number;
}> {
  const rows = new Map<
    string,
    {
      id: string;
      cwd: string;
      harness: HarnessId;
      title: string;
      updatedAt: number;
    }
  >();
  for (const row of history) {
    rows.set(row.id, {
      id: row.id,
      cwd: row.cwd,
      harness: row.harness,
      title: row.title,
      updatedAt: row.updatedAt,
    });
  }
  for (const session of sessions) {
    const previous = rows.get(session.id);
    rows.set(session.id, {
      id: session.id,
      cwd: session.cwd,
      harness: session.harness,
      title: session.title,
      updatedAt: previous?.updatedAt ?? Date.now(),
    });
  }
  return [...rows.values()];
}

export function mergeHits(...lists: AppSearchHit[][]): AppSearchHit[] {
  const byId = new Map<string, AppSearchHit>();
  for (const list of lists) {
    for (const hit of list) {
      const existing = byId.get(hit.id);
      if (!existing) {
        byId.set(hit.id, hit);
        continue;
      }
      if (scoreOf(hit) > scoreOf(existing)) byId.set(hit.id, hit);
    }
  }
  return [...byId.values()];
}

export function filterHitsByProject(
  hits: AppSearchHit[],
  cwd: string | undefined,
): AppSearchHit[] {
  if (!cwd || cwd === "~") return hits;
  return hits.filter((hit) => {
    if (hit.kind === "file" || hit.kind === "content") return true;
    if (hit.kind === "project") return sameProjectPath(hit.path, cwd);
    return sameProjectPath(hit.cwd, cwd);
  });
}

export function groupHits(
  hits: AppSearchHit[],
  scope: SearchScope,
): GroupedHits {
  const grouped: GroupedHits = {
    conversations: [],
    messages: [],
    files: [],
    content: [],
    projects: [],
  };
  for (const hit of hits) {
    if (hit.kind === "conversation") grouped.conversations.push(hit);
    else if (hit.kind === "message") grouped.messages.push(hit);
    else if (hit.kind === "file") grouped.files.push(hit);
    else if (hit.kind === "content") grouped.content.push(hit);
    else grouped.projects.push(hit);
  }
  grouped.conversations.sort(byScoreThenRecency);
  grouped.messages.sort(byScoreThenRecency);
  grouped.files.sort((a, b) => b.score - a.score || a.relative.localeCompare(b.relative));
  grouped.projects.sort((a, b) => b.score - a.score || a.name.localeCompare(b.name));

  const limits = SCOPE_LIMITS[scope];
  return {
    conversations: grouped.conversations.slice(0, limits.conversations),
    messages: grouped.messages.slice(0, limits.messages),
    files: grouped.files.slice(0, limits.files),
    content: grouped.content.slice(0, limits.content),
    projects: grouped.projects.slice(0, limits.projects),
  };
}

export function flattenGrouped(grouped: GroupedHits): AppSearchHit[] {
  return [
    ...grouped.conversations,
    ...grouped.messages,
    ...grouped.files,
    ...grouped.content,
    ...grouped.projects,
  ];
}

export function groupedCount(grouped: GroupedHits): number {
  return flattenGrouped(grouped).length;
}

function isSearchableRole(role: Block["role"]): boolean {
  return (
    role === "user" ||
    role === "assistant" ||
    role === "tool" ||
    role === "tasks" ||
    role === "plan"
  );
}

function blockSearchText(block: Block): string {
  const parts: string[] = [];
  if (block.text.trim()) parts.push(block.text);
  if (block.tool?.title) parts.push(block.tool.title);
  if (block.tool?.detail) parts.push(block.tool.detail);
  if (block.tool?.preview?.query) parts.push(block.tool.preview.query);
  if (block.tool?.preview?.path) parts.push(block.tool.preview.path);
  if (block.tool?.preview?.output) parts.push(block.tool.preview.output);
  if (block.tool?.preview?.title) parts.push(block.tool.preview.title);
  return parts.join("\n");
}

function recencyBonus(updatedAt: number): number {
  if (!Number.isFinite(updatedAt) || updatedAt <= 0) return 0;
  const age = Date.now() - updatedAt;
  if (age <= 0) return 24;
  const week = 7 * 24 * 60 * 60 * 1000;
  return Math.max(0, Math.round(24 * (1 - Math.min(age, week) / week)));
}

function scoreOf(hit: AppSearchHit): number {
  if (hit.kind === "content") return 0;
  return hit.score;
}

function byScoreThenRecency<T extends { score: number; updatedAt: number }>(
  a: T,
  b: T,
): number {
  if (b.score !== a.score) return b.score - a.score;
  return b.updatedAt - a.updatedAt;
}
