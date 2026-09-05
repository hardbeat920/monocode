import { invoke } from "@tauri-apps/api/core";
import { fuzzyMatch } from "./fuzzy";
import type { ProjectFile } from "./fs";
import type { RankedFile } from "./fileIndex";
import { t } from "./i18n";
import { projectName } from "./paths";
import { looksLikeProject } from "./recents";

export const NOTE_MENTION_PREFIX = "note/";
export const NOTE_PATH_PREFIX = "note:";

export type Note = {
  id: string;
  slug: string;
  title: string;
  body: string;
  sourceSessionId?: string;
  sourceCwd?: string;
  createdAt: number;
  updatedAt: number;
};

export type NoteUpsert = {
  id: string;
  title: string;
  body: string;
  sourceSessionId?: string;
  sourceCwd?: string;
};

/** Note chip shown in the composer and on the user turn in the thread. */
export type NoteCardMeta = {
  id: string;
  slug: string;
  title: string;
  sourceCwd?: string;
};

/** Composer chip: display fields plus the body injected into the harness prompt. */
export type NoteComposerCard = NoteCardMeta & {
  body: string;
};

export function noteCardMeta(card: NoteComposerCard): NoteCardMeta {
  return {
    id: card.id,
    slug: card.slug,
    title: card.title,
    ...(card.sourceCwd ? { sourceCwd: card.sourceCwd } : {}),
  };
}

export const ADD_NOTE_TO_CHAT_EVENT = "monocode:add-note-to-chat";

const MAX_TITLE = 200;
const MAX_NOTE_PICKER = 8;
const NOTE_SLUG_RE = /(^|\s)@note\/([A-Za-z0-9_-]+)/g;

let cache: Note[] | null = null;
let inflight: Promise<Note[]> | null = null;

export function peekNotes(): Note[] | null {
  return cache;
}

export function invalidateNotes() {
  cache = null;
}

export async function loadNotes(refresh = false): Promise<Note[]> {
  if (!refresh && cache) return cache;
  if (!refresh && inflight) return inflight;

  const promise = invoke<Note[]>("notes_list")
    .then((notes) => {
      cache = notes;
      return notes;
    })
    .catch(() => {
      if (!cache) cache = [];
      return cache;
    })
    .finally(() => {
      if (inflight === promise) inflight = null;
    });
  inflight = promise;
  return promise;
}

export async function getNote(id: string): Promise<Note | null> {
  const note = await invoke<Note | null>("notes_get", { id });
  return note;
}

export async function upsertNote(note: NoteUpsert): Promise<Note> {
  const saved = await invoke<Note>("notes_upsert", { note });
  cache = null;
  return saved;
}

export async function deleteNote(id: string): Promise<void> {
  await invoke("notes_delete", { id });
  cache = null;
}

export async function createNote(input: {
  title?: string;
  body?: string;
  sourceSessionId?: string;
  sourceCwd?: string;
}): Promise<Note> {
  const body = (input.body ?? "").replace(/\r\n?/g, "\n");
  return upsertNote({
    id: crypto.randomUUID(),
    title: (input.title ?? noteTitle(body)).slice(0, MAX_TITLE),
    body,
    ...(input.sourceSessionId ? { sourceSessionId: input.sourceSessionId } : {}),
    ...(input.sourceCwd ? { sourceCwd: input.sourceCwd } : {}),
  });
}

export function isNoteMentionPath(path: string): boolean {
  return path.startsWith(NOTE_PATH_PREFIX);
}

export function noteMentionLabel(note: Note): string {
  return `${NOTE_MENTION_PREFIX}${note.slug}`;
}

export function notesAsProjectFiles(notes: Note[]): ProjectFile[] {
  return notes.map((note) => ({
    name: note.title,
    path: `${NOTE_PATH_PREFIX}${note.id}`,
    relative: noteMentionLabel(note),
  }));
}

export function rankNoteFiles(
  notes: Note[],
  query: string,
  limit = MAX_NOTE_PICKER,
): RankedFile[] {
  const needle = query.replace(/\/+$/, "").trim().toLowerCase();
  if (!needle) {
    return [...notes]
      .sort((a, b) => b.updatedAt - a.updatedAt)
      .slice(0, limit)
      .map((note) => ({
        ...notesAsProjectFiles([note])[0]!,
        score: 0,
        positions: [],
      }));
  }

  const scored: { note: Note; score: number; positions: number[] }[] = [];
  for (const note of notes) {
    const titleHit = fuzzyMatch(needle, note.title);
    const slugHit = titleHit ? null : fuzzyMatch(needle, note.slug);
    const hit = titleHit ?? slugHit;
    if (!hit) continue;
    scored.push({
      note,
      score: titleHit ? hit.score + 400 : hit.score,
      positions: titleHit ? hit.positions : [],
    });
  }
  scored.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    return b.note.updatedAt - a.note.updatedAt;
  });
  return scored.slice(0, limit).map((row) => ({
    ...notesAsProjectFiles([row.note])[0]!,
    score: row.score,
    positions: row.positions,
  }));
}

/** First markdown heading, or the first non-empty prose line. */
export function noteTitle(text: string): string {
  const heading = text.match(/^\s{0,3}#{1,6}\s+(.+)$/m);
  if (heading) {
    const title = unwrapMarkdown(heading[1]).slice(0, MAX_TITLE);
    if (title) return title;
  }
  let inFence = false;
  for (const line of text.split(/\r?\n/)) {
    if (/^\s*```/.test(line)) {
      inFence = !inFence;
      continue;
    }
    if (inFence) continue;
    const trimmed = line.trim();
    if (!trimmed || trimmed === "---") continue;
    const title = unwrapMarkdown(trimmed).slice(0, MAX_TITLE);
    if (title) return title;
  }
  return t("Untitled");
}

/** Folder name for a note saved from a session, or null when there is no project. */
export function noteSourceProject(cwd?: string): string | null {
  if (!cwd || !looksLikeProject(cwd)) return null;
  const name = projectName(cwd);
  return name || null;
}

export function notePreview(text: string, title: string): string {
  const parts: string[] = [];
  let inFence = false;
  for (const line of text.split(/\r?\n/)) {
    if (/^\s*```/.test(line)) {
      inFence = !inFence;
      continue;
    }
    if (inFence || /^\s{0,3}#{1,6}\s+/.test(line)) continue;
    const trimmed = line.trim();
    if (!trimmed || trimmed === "---") continue;
    const next = unwrapMarkdown(trimmed.replace(/^[-*+]\s+/, ""));
    if (!next || next === title) continue;
    parts.push(next);
    if (parts.join(" ").length >= 120) break;
  }
  const preview = parts.join(" ");
  if (!preview) return "";
  return preview.length > 120 ? `${preview.slice(0, 119)}…` : preview;
}

export function appendNoteReference(
  draft: string,
  title: string,
  body: string,
): string {
  const content = body.replace(/\r\n?/g, "\n").trim();
  if (!content) return draft;
  const heading = title.trim() || t("Untitled");
  const block = `${t("Note: {0}", [heading])}\n\n${content}`;
  const separator =
    draft.length === 0
      ? ""
      : draft.endsWith("\n\n")
        ? ""
        : draft.endsWith("\n")
          ? "\n"
          : "\n\n";
  return `${draft}${separator}${block}\n\n`;
}

export function noteSlugsInText(text: string): string[] {
  const found: string[] = [];
  const seen = new Set<string>();
  NOTE_SLUG_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = NOTE_SLUG_RE.exec(text))) {
    const slug = match[2];
    if (!slug || seen.has(slug)) continue;
    seen.add(slug);
    found.push(slug);
  }
  return found;
}

export function injectNotePrompt(text: string, notes: Note[]): string {
  if (notes.length === 0) return text;
  const blocks = notes.map((note) => {
    const heading = note.title.trim() || t("Untitled");
    return [t('Referenced note "{0}":', [heading]), "", note.body.trim()].join("\n");
  });
  return [text.trimEnd(), "", "---", ...blocks].join("\n");
}

export async function applyNotesToTurn(text: string): Promise<string> {
  const slugs = noteSlugsInText(text);
  if (slugs.length === 0) return text;
  const notes = await loadNotes();
  const picked: Note[] = [];
  const seen = new Set<string>();
  for (const slug of slugs) {
    const note = notes.find((item) => item.slug === slug);
    if (!note || seen.has(note.id)) continue;
    seen.add(note.id);
    picked.push(note);
  }
  if (picked.length === 0) return text;
  return injectNotePrompt(text, picked);
}

export function noteComposerCard(note: Note): NoteComposerCard {
  return {
    id: note.id,
    slug: note.slug,
    title: note.title,
    body: note.body,
    ...(note.sourceCwd ? { sourceCwd: note.sourceCwd } : {}),
  };
}

export function requestAddNoteToChat(note: Note) {
  if (typeof window === "undefined") return;
  if (!note.body.replace(/\r\n?/g, "\n").trim()) return;
  window.dispatchEvent(
    new CustomEvent<NoteComposerCard>(ADD_NOTE_TO_CHAT_EVENT, {
      detail: noteComposerCard(note),
    }),
  );
}

export function composeNoteMessage(
  card: NoteComposerCard | undefined,
  text: string,
): string {
  if (!card) return text.trim();
  const lead = text.trim() || t("Use this note.");
  return injectNotePrompt(lead, [
    {
      id: card.id,
      slug: card.slug,
      title: card.title,
      body: card.body,
      createdAt: 0,
      updatedAt: 0,
    },
  ]);
}

function unwrapMarkdown(value: string): string {
  return value
    .replace(/!\[[^\]]*]\([^)]*\)/g, "")
    .replace(/\[([^\]]+)]\([^)]*\)/g, "$1")
    .replace(/[*_`]+/g, "")
    .trim();
}
