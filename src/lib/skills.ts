import {
  createPath,
  homeDir,
  listSkills,
  readTextFile,
  writeTextFile,
  type DiscoveredSkill,
} from "./fs";
import { invalidateProjectFiles } from "./fileIndex";
import { fuzzyMatch } from "./fuzzy";
import { joinPath } from "./paths";
import { looksLikeProject, normalizeProjectPath } from "./recents";
import { isMarkdownBlockquotePosition } from "./quoteDraft";
import type { HarnessId } from "./session";
import { discoverPiSkills } from "./harness/piSkills";
import {
  CREATE_SKILL_BODY,
  CREATE_SKILL_DESCRIPTION,
  CREATE_SKILL_NAME,
} from "./createSkill";

export type SkillScope = "project" | "user" | "builtin";
export type SkillSource =
  | "agents"
  | "claude"
  | "cursor"
  | "codex"
  | "opencode"
  | "pi"
  | "omp"
  | "fx"
  | "monocode";

type SkillCommon = {
  name: string;
  description: string;
  invocation: string;
};

export type FileSkill = SkillCommon & {
  kind: "file";
  path: string;
  scope: Exclude<SkillScope, "builtin">;
  source: SkillSource;
};

export type BuiltinSkill = SkillCommon & {
  kind: "builtin";
  scope: "builtin";
  source: "monocode";
};

export type NativeSkill = SkillCommon & {
  kind: "native";
  source: "pi";
};

export type Skill = FileSkill | BuiltinSkill | NativeSkill;

export type SlashToken = {
  start: number;
  end: number;
  query: string;
};

export const BUILTIN_CREATE_SKILL: BuiltinSkill = {
  kind: "builtin",
  name: CREATE_SKILL_NAME,
  description: CREATE_SKILL_DESCRIPTION,
  invocation: CREATE_SKILL_NAME,
  scope: "builtin",
  source: "monocode",
};

const SKILL_NAME_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const SKILL_TOKEN_RE =
  /(^|\s)\/([a-z0-9]+(?:-[a-z0-9]+)*(?::[a-z0-9]+(?:-[a-z0-9]+)*)?)(?=\s|$)/g;
const MAX_PICKER = 50;
const PI_SKILL_TTL_MS = 30_000;
const PI_SKILL_RETRY_MS = 5_000;

export type SkillCatalogContext = {
  harness: HarnessId;
  cwd: string;
};

type CatalogRequest = {
  generation: number;
  promise: Promise<Skill[]>;
};

type CatalogEntry = {
  cwd: string;
  skills: Skill[] | null;
  loadedAt: number;
  retryAt: number;
  generation: number;
  inFlight: CatalogRequest | null;
};

const catalogEntries = new Map<string, CatalogEntry>();

export function skillCatalogKey(context: SkillCatalogContext): string {
  return `${context.harness}\0${normalizeProjectPath(context.cwd)}`;
}

export function peekSkills(context: SkillCatalogContext): Skill[] | null {
  return catalogEntries.get(skillCatalogKey(context))?.skills ?? null;
}

export function invalidateSkills(context?: { cwd: string }) {
  if (!context) {
    catalogEntries.clear();
    return;
  }
  const cwd = normalizeProjectPath(context.cwd);
  for (const entry of catalogEntries.values()) {
    if (entry.cwd !== cwd) continue;
    entry.generation += 1;
    entry.loadedAt = 0;
    entry.retryAt = 0;
    entry.inFlight = null;
  }
}

export function loadSkills(
  context: SkillCatalogContext,
  options?: { refresh?: boolean },
): Promise<Skill[]> {
  const normalized = {
    harness: context.harness,
    cwd: normalizeProjectPath(context.cwd),
  } satisfies SkillCatalogContext;
  const key = skillCatalogKey(normalized);
  let entry = catalogEntries.get(key);
  if (!entry) {
    entry = {
      cwd: normalized.cwd,
      skills: null,
      loadedAt: 0,
      retryAt: 0,
      generation: 0,
      inFlight: null,
    };
    catalogEntries.set(key, entry);
  }

  const now = Date.now();
  if (options?.refresh) {
    if (entry.inFlight?.generation === entry.generation) {
      return entry.inFlight.promise;
    }
    entry.generation += 1;
    entry.retryAt = 0;
    return startCatalogLoad(key, entry, normalized);
  }

  if (entry.inFlight?.generation === entry.generation) {
    return entry.inFlight.promise;
  }
  if (normalized.harness !== "pi" && entry.skills) {
    return Promise.resolve(entry.skills);
  }
  if (
    normalized.harness === "pi" &&
    entry.skills &&
    now - entry.loadedAt < PI_SKILL_TTL_MS
  ) {
    return Promise.resolve(entry.skills);
  }
  if (normalized.harness === "pi" && now < entry.retryAt) {
    return Promise.resolve(entry.skills ?? []);
  }
  return startCatalogLoad(key, entry, normalized);
}

function startCatalogLoad(
  key: string,
  entry: CatalogEntry,
  context: SkillCatalogContext,
): Promise<Skill[]> {
  const generation = entry.generation;
  const promise = loadCatalog(context)
    .then((skills) => {
      if (
        catalogEntries.get(key) !== entry ||
        entry.generation !== generation
      ) {
        return catalogEntries.get(key)?.skills ?? [];
      }
      entry.skills = skills;
      entry.loadedAt = Date.now();
      entry.retryAt = 0;
      return skills;
    })
    .catch(() => {
      if (
        catalogEntries.get(key) !== entry ||
        entry.generation !== generation
      ) {
        return catalogEntries.get(key)?.skills ?? [];
      }
      if (context.harness === "pi") {
        entry.retryAt = Date.now() + PI_SKILL_RETRY_MS;
        return entry.skills ?? [];
      }
      const fallback = mergeCatalog([]);
      entry.skills = fallback;
      entry.loadedAt = Date.now();
      return fallback;
    })
    .finally(() => {
      if (
        catalogEntries.get(key) === entry &&
        entry.generation === generation &&
        entry.inFlight?.promise === promise
      ) {
        entry.inFlight = null;
      }
    });
  entry.inFlight = { generation, promise };
  return promise;
}

async function loadCatalog(context: SkillCatalogContext): Promise<Skill[]> {
  if (context.harness === "pi") {
    const commands = await discoverPiSkills(context.cwd);
    return commands.map((command): NativeSkill => ({
      kind: "native",
      ...command,
    }));
  }
  return mergeCatalog(await listSkills(context.cwd));
}

export function mergeCatalog(discovered: DiscoveredSkill[]): Skill[] {
  const out = new Map<string, Skill>();
  const add = (skill: Skill) => {
    if (!skill.name || out.has(skill.name)) return;
    out.set(skill.name, skill);
  };
  for (const skill of discovered) {
    if (skill.source === "agents") add(asSkill(skill));
  }
  add(BUILTIN_CREATE_SKILL);
  for (const skill of discovered) {
    if (skill.source !== "agents") add(asSkill(skill));
  }
  return [...out.values()];
}

function asSkill(skill: DiscoveredSkill): FileSkill {
  return {
    kind: "file",
    name: skill.name,
    description: skill.description,
    invocation: skill.name,
    path: skill.path,
    scope: skill.scope === "user" ? "user" : "project",
    source: skill.source === "monocode" ? "monocode" : skill.source,
  };
}

export function rankSkills(skills: Skill[], query: string, limit = MAX_PICKER): Skill[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return [...skills]
      .sort((a, b) => {
        const rank = scopeRank(a) - scopeRank(b);
        if (rank !== 0) return rank;
        return a.name.localeCompare(b.name);
      })
      .slice(0, limit);
  }

  const scored: { skill: Skill; score: number }[] = [];
  for (const skill of skills) {
    const nameHit = fuzzyMatch(needle, skill.name);
    const invocationHit = nameHit
      ? null
      : fuzzyMatch(needle, skill.invocation);
    const descHit =
      nameHit || invocationHit
        ? null
        : fuzzyMatch(needle, skill.description);
    const hit = nameHit ?? invocationHit ?? descHit;
    if (!hit) continue;
    const score = nameHit || invocationHit ? hit.score + 400 : hit.score;
    scored.push({ skill, score });
  }
  scored.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    return a.skill.name.localeCompare(b.skill.name);
  });
  return scored.slice(0, limit).map((row) => row.skill);
}

function scopeRank(skill: Skill): number {
  if (skill.kind === "builtin") return 0;
  if (skill.kind === "native" || skill.scope === "project") return 1;
  return 2;
}

/** Slash token that contains `cursor`, if the user is typing `/skill`. */
export function slashTokenAt(text: string, cursor: number): SlashToken | null {
  const i = clamp(cursor, 0, text.length);
  let start = i;
  while (start > 0 && !isSpace(text[start - 1]!)) start -= 1;
  if (text[start] !== "/") return null;
  if (start > 0 && text[start - 1] === ":") return null;
  if (isMarkdownBlockquotePosition(text, start)) return null;

  let end = start + 1;
  while (end < text.length && !isSpace(text[end]!)) end += 1;

  const typed = text.slice(start + 1, i);
  if (typed.includes("/") || typed.includes("\\")) return null;
  if (/[A-Z]/.test(typed)) return null;
  if (!/^(?:[a-z0-9-]+(?::[a-z0-9-]*)?)?$/.test(typed)) return null;

  return { start, end, query: typed };
}

export function replaceSlashToken(
  text: string,
  token: SlashToken,
  name: string,
): string {
  const rest = text.slice(token.end);
  const spacer = rest.startsWith(" ") ? "" : " ";
  return `${text.slice(0, token.start)}/${name}${spacer}${rest}`;
}

export function skillNamesInText(text: string): string[] {
  const names: string[] = [];
  const seen = new Set<string>();
  SKILL_TOKEN_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = SKILL_TOKEN_RE.exec(text))) {
    const name = match[2];
    const start = match.index + (match[1]?.length ?? 0);
    if (!name || seen.has(name) || isMarkdownBlockquotePosition(text, start)) {
      continue;
    }
    seen.add(name);
    names.push(name);
  }
  return names;
}

export type SkillTextPart = {
  text: string;
  skill: boolean;
};

/** Split composer text so known `/skill` tokens can be highlighted. */
export function skillTextParts(
  text: string,
  names: ReadonlySet<string>,
): SkillTextPart[] {
  if (!text) return [];
  if (names.size === 0) return [{ text, skill: false }];

  const parts: SkillTextPart[] = [];
  const push = (value: string, skill: boolean) => {
    if (!value) return;
    const last = parts[parts.length - 1];
    if (last && last.skill === skill) {
      last.text += value;
      return;
    }
    parts.push({ text: value, skill });
  };

  SKILL_TOKEN_RE.lastIndex = 0;
  let cursor = 0;
  let match: RegExpExecArray | null;
  while ((match = SKILL_TOKEN_RE.exec(text))) {
    const name = match[2];
    const lead = match[1] ?? "";
    const start = match.index + lead.length;
    if (
      !name ||
      !names.has(name) ||
      isMarkdownBlockquotePosition(text, start)
    ) {
      continue;
    }
    const end = start + 1 + name.length;
    push(text.slice(cursor, start), false);
    push(text.slice(start, end), true);
    cursor = end;
  }
  push(text.slice(cursor), false);
  return parts;
}

export function injectSkillPrompt(
  text: string,
  skills: Skill[],
  bodies: Record<string, string>,
): string {
  const blocks: string[] = [];
  const seen = new Set<string>();
  for (const skill of skills) {
    if (seen.has(skill.name)) continue;
    seen.add(skill.name);
    const body = bodies[skill.name]?.trim();
    if (!body) continue;
    blocks.push(`## /${skill.name}\n\n${body}`);
  }
  if (blocks.length === 0) return text;
  return [
    "The user invoked skill(s) with /name. Follow every instruction in each skill body.",
    "",
    blocks.join("\n\n"),
    "",
    "---",
    "",
    text,
  ].join("\n");
}

export async function applySkillsToTurn(
  text: string,
  context: SkillCatalogContext,
): Promise<string> {
  if (context.harness === "pi") return text;
  const names = skillNamesInText(text);
  if (names.length === 0) return text;
  const catalog = await loadSkills(context);
  const picked: Array<FileSkill | BuiltinSkill> = [];
  for (const name of names) {
    const skill = catalog.find((item) => item.name === name);
    if (skill?.kind === "file" || skill?.kind === "builtin") {
      picked.push(skill);
    }
  }
  if (picked.length === 0) return text;
  const bodies: Record<string, string> = {};
  await Promise.all(
    picked.map(async (skill) => {
      bodies[skill.name] = await readSkillBody(skill);
    }),
  );
  return injectSkillPrompt(text, picked, bodies);
}

type SkillLoader = typeof loadSkills;

export function warmPiSkills(
  context: SkillCatalogContext,
  load: SkillLoader = loadSkills,
): void {
  if (context.harness !== "pi") return;
  void load(context).catch(() => undefined);
}

export async function readSkillBody(
  skill: FileSkill | BuiltinSkill,
): Promise<string> {
  if (skill.kind === "builtin") return CREATE_SKILL_BODY;
  try {
    return await readTextFile(skill.path);
  } catch {
    return `Skill "${skill.name}" could not be read from ${skill.path}.`;
  }
}

export function slugSkillName(raw: string): string {
  return raw
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64)
    .replace(/-+$/g, "");
}

export function isValidSkillName(name: string): boolean {
  return SKILL_NAME_RE.test(name) && name.length <= 64;
}

export function titleFromSkillName(name: string): string {
  return name
    .split("-")
    .filter(Boolean)
    .map((part) => part.slice(0, 1).toUpperCase() + part.slice(1))
    .join(" ");
}

export function blankSkillMarkdown(name: string): string {
  const title = titleFromSkillName(name);
  const words = name.replace(/-/g, " ");
  return `---
name: ${name}
description: ${title}. Use when the user asks to ${words}.
---

# ${title}

## Instructions

`;
}

export async function createBlankSkill(input: {
  cwd: string;
  name: string;
  scope: "project" | "user";
}): Promise<string> {
  const name = slugSkillName(input.name);
  if (!isValidSkillName(name)) {
    throw new Error("Use a lowercase name with letters, numbers, and hyphens.");
  }
  const root =
    input.scope === "user" || !looksLikeProject(input.cwd)
      ? await homeDir()
      : input.cwd;
  const relative = `.agents/skills/${name}`;
  await createPath(root, relative, true);
  const path = joinPath(root, `${relative}/SKILL.md`);
  await writeTextFile(path, blankSkillMarkdown(name));
  invalidateSkills({ cwd: input.cwd });
  invalidateProjectFiles(input.cwd);
  return path;
}

function isSpace(ch: string): boolean {
  return ch === " " || ch === "\n" || ch === "\t" || ch === "\r";
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
