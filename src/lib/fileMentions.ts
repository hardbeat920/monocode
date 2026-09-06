import { loadProjectFiles, rankProjectFiles, type RankedFile } from "./fileIndex";
import type { ProjectFile } from "./fs";
import { isMarkdownBlockquotePosition } from "./quoteDraft";

export type MentionToken = {
  start: number;
  end: number;
  query: string;
};

/** `@label` → the project file it points at, plus the label to write. */
export type MentionIndex = {
  /** Every writable label (basename when unique, always the relative path). */
  labels: Map<string, ProjectFile>;
  /** Preferred label per file path — the shortest unambiguous one. */
  labelOf: Map<string, string>;
};

export type MentionHit = {
  start: number;
  end: number;
  label: string;
  file: ProjectFile;
};

export type MentionTextPart = {
  text: string;
  file?: ProjectFile;
};

const MENTION_TOKEN_RE = /(^|\s)@(\S+)/g;
const TRAILING_PUNCTUATION = new Set([
  ",",
  ";",
  ":",
  "!",
  "?",
  ")",
  "]",
  "}",
  '"',
  "'",
  "/",
]);
const MAX_QUERY = 120;
const MAX_PICKER = 30;

/** Mention token that contains `cursor`, if the user is typing `@file`. */
export function mentionTokenAt(
  text: string,
  cursor: number,
): MentionToken | null {
  const i = clamp(cursor, 0, text.length);
  let start = i;
  while (start > 0 && !isSpace(text[start - 1]!)) start -= 1;
  if (text[start] !== "@") return null;
  if (isMarkdownBlockquotePosition(text, start)) return null;

  let end = start + 1;
  while (end < text.length && !isSpace(text[end]!)) end += 1;

  const typed = text.slice(start + 1, i);
  if (typed.includes("@") || typed.length > MAX_QUERY) return null;

  return { start, end, query: typed };
}

export function replaceMentionToken(
  text: string,
  token: MentionToken,
  label: string,
): string {
  const rest = text.slice(token.end);
  const spacer = rest.startsWith(" ") ? "" : " ";
  return `${text.slice(0, token.start)}@${label}${spacer}${rest}`;
}

export function buildMentionIndex(files: ProjectFile[]): MentionIndex {
  const entries = withMentionDirectories(files).filter((file) =>
    isMentionableRelative(file.relative),
  );
  const counts = new Map<string, number>();
  for (const file of entries) {
    counts.set(file.name, (counts.get(file.name) ?? 0) + 1);
  }

  const labels = new Map<string, ProjectFile>();
  const labelOf = new Map<string, string>();
  const used = new Set<string>();

  const claim = (label: string, file: ProjectFile, preferred = false) => {
    if (!isTokenSafe(label) || used.has(label)) return false;
    used.add(label);
    labels.set(label, file);
    if (preferred || !labelOf.has(file.path)) labelOf.set(file.path, label);
    return true;
  };

  for (const file of entries) {
    const unique =
      !file.isDir && counts.get(file.name) === 1 && isTokenSafe(file.name);
    if (unique) claim(file.name, file, true);
    if (isTokenSafe(file.relative)) claim(file.relative, file);
  }

  for (const file of entries) {
    if (labelOf.has(file.path)) continue;
    const encoded = encodeMentionToken(file.relative);
    if (!encoded) continue;
    let label = encoded;
    for (let n = 2; used.has(label) && n < 1000; n++) {
      label = `${encoded}~${n}`;
    }
    claim(label, file, true);
  }

  return { labels, labelOf };
}

export function mentionLabel(file: ProjectFile, index: MentionIndex): string {
  return index.labelOf.get(file.path) ?? file.relative;
}

/** Files the picker offers: recents first without a query, fuzzy after. */
export function rankMentionFiles(
  files: ProjectFile[],
  query: string,
  recents: string[],
  limit = MAX_PICKER,
): RankedFile[] {
  const usable = withMentionDirectories(files).filter((file) =>
    isMentionableRelative(file.relative),
  );
  const needle = query.replace(/\/+$/, "").trim();
  if (needle) return rankProjectFiles(usable, needle, recents, limit);

  const byPath = new Map(usable.map((file) => [file.path, file]));
  const out: RankedFile[] = [];
  const seen = new Set<string>();
  for (const path of recents) {
    const file = byPath.get(path);
    if (!file || seen.has(path)) continue;
    seen.add(path);
    out.push({ ...file, score: 0, positions: [] });
    if (out.length >= limit) return out;
  }

  const rest = [...usable].sort((a, b) => {
    const depth = pathDepth(a.relative) - pathDepth(b.relative);
    if (depth !== 0) return depth;
    const dir = Number(Boolean(b.isDir)) - Number(Boolean(a.isDir));
    if (dir !== 0) return dir;
    return a.relative.localeCompare(b.relative);
  });
  for (const file of rest) {
    if (seen.has(file.path)) continue;
    seen.add(file.path);
    out.push({ ...file, score: 0, positions: [] });
    if (out.length >= limit) break;
  }
  return out;
}

/** Split composer text so known `@file` tokens can be highlighted. */
export function fileMentionParts(
  text: string,
  labels: ReadonlyMap<string, ProjectFile>,
): MentionTextPart[] {
  if (!text) return [];
  const hits = scanMentions(text, labels);
  if (hits.length === 0) return [{ text }];

  const parts: MentionTextPart[] = [];
  let cursor = 0;
  for (const hit of hits) {
    if (hit.start > cursor) parts.push({ text: text.slice(cursor, hit.start) });
    parts.push({ text: text.slice(hit.start, hit.end), file: hit.file });
    cursor = hit.end;
  }
  if (cursor < text.length) parts.push({ text: text.slice(cursor) });
  return parts;
}

export function fileMentionsInText(
  text: string,
  labels: ReadonlyMap<string, ProjectFile>,
): MentionHit[] {
  const seen = new Set<string>();
  return scanMentions(text, labels).filter((hit) => {
    if (seen.has(hit.file.path)) return false;
    seen.add(hit.file.path);
    return true;
  });
}

/**
 * Spell out where each `@name` lives, so the harness does not have to guess
 * which `App.tsx` the user meant. Tokens already written as a project-relative
 * path need no help.
 */
export async function applyFileMentionsToTurn(
  text: string,
  cwd: string,
): Promise<string> {
  if (!looksMentioned(text)) return text;
  const files = await loadProjectFiles(cwd).catch(() => []);
  if (files.length === 0) return text;

  const index = buildMentionIndex(files);
  const lines = fileMentionsInText(text, index.labels)
    .filter((hit) => hit.label !== hit.file.relative)
    .map((hit) => `- @${hit.label} → ${hit.file.relative}`);
  if (lines.length === 0) return text;

  return [text, "", "---", "Referenced with @ above:", ...lines].join("\n");
}

/** Parent folders of indexed files, so `@` can point at a directory. */
export function withMentionDirectories(files: ProjectFile[]): ProjectFile[] {
  const dirs = new Map<string, ProjectFile>();
  for (const file of files) {
    if (file.isDir) {
      if (isSafeProjectRelative(file.relative)) dirs.set(file.relative, file);
      continue;
    }
    if (!isSafeProjectRelative(file.relative)) continue;
    const parts = file.relative.split("/");
    let rel = "";
    for (let i = 0; i < parts.length - 1; i++) {
      const segment = parts[i]!;
      rel = rel ? `${rel}/${segment}` : segment;
      if (dirs.has(rel) || !isSafeProjectRelative(rel)) continue;
      dirs.set(rel, {
        name: segment,
        path: mentionDirPath(file, rel),
        relative: rel,
        isDir: true,
      });
    }
  }
  if (dirs.size === 0) return files;

  const seen = new Set(files.map((file) => file.path));
  const extra = [...dirs.values()].filter((dir) => !seen.has(dir.path));
  return extra.length === 0 ? files : [...files, ...extra];
}

function scanMentions(
  text: string,
  labels: ReadonlyMap<string, ProjectFile>,
): MentionHit[] {
  if (labels.size === 0) return [];
  const hits: MentionHit[] = [];
  MENTION_TOKEN_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = MENTION_TOKEN_RE.exec(text))) {
    const raw = match[2];
    if (!raw) continue;
    const start = match.index + (match[1] ?? "").length;
    if (isMarkdownBlockquotePosition(text, start)) continue;
    const resolved = resolveLabel(raw, labels);
    if (!resolved) continue;
    hits.push({
      start,
      end: start + 1 + resolved.label.length,
      label: resolved.label,
      file: resolved.file,
    });
  }
  return hits;
}

/** `@App.tsx,` still points at `App.tsx` — peel trailing punctuation. */
function resolveLabel(
  raw: string,
  labels: ReadonlyMap<string, ProjectFile>,
): { label: string; file: ProjectFile } | null {
  let value = raw;
  while (value) {
    const file = labels.get(value);
    if (file) return { label: value, file };
    if (!TRAILING_PUNCTUATION.has(value[value.length - 1]!)) return null;
    value = value.slice(0, -1);
  }
  return null;
}

function looksMentioned(text: string): boolean {
  return /(^|\s)@\S/.test(text);
}

function mentionDirPath(file: ProjectFile, relativeDir: string): string {
  const trail = file.relative.slice(relativeDir.length);
  if (!trail) return file.path;
  if (file.path.endsWith(trail)) {
    return file.path.slice(0, file.path.length - trail.length);
  }
  const winTrail = trail.replace(/\//g, "\\");
  if (file.path.endsWith(winTrail)) {
    return file.path.slice(0, file.path.length - winTrail.length);
  }
  return file.path.slice(0, file.path.length - trail.length);
}

function pathDepth(relative: string): number {
  let depth = 0;
  for (const ch of relative) if (ch === "/") depth += 1;
  return depth;
}

/** Workspace-relative only: no absolute paths, `..`, or control / format characters. */
function isSafeProjectRelative(relative: string): boolean {
  if (!relative || relative.length > MAX_QUERY * 4) return false;
  if (hasUnsafeChars(relative)) return false;
  if (relative.startsWith("/") || relative.startsWith("\\")) return false;
  if (relative.includes("://")) return false;
  for (const part of relative.split("/")) {
    if (!part || part === "." || part === "..") return false;
  }
  return true;
}

/**
 * `@` tokens are one whitespace-delimited word. Collapse whitespace so a
 * spaced path still inserts as a normal `@label`; the real relative path is
 * recovered from the mention index on send.
 */
function encodeMentionToken(relative: string): string | null {
  const encoded = relative.replace(/\s+/g, "-");
  return isTokenSafe(encoded) ? encoded : null;
}

function isTokenSafe(value: string): boolean {
  return (
    value.length > 0 &&
    value.length <= MAX_QUERY &&
    !/[\s@\\]/.test(value) &&
    !hasUnsafeChars(value)
  );
}

/** ASCII/C1 controls, bidi/format marks, and Unicode line/paragraph separators. */
function hasUnsafeChars(value: string): boolean {
  return /[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]/u.test(value);
}

function isMentionableRelative(relative: string): boolean {
  if (!isSafeProjectRelative(relative)) return false;
  return isTokenSafe(relative) || encodeMentionToken(relative) != null;
}

function isSpace(ch: string): boolean {
  return ch === " " || ch === "\n" || ch === "\t" || ch === "\r";
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
