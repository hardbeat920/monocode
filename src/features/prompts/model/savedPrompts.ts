import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { fuzzyMatch } from "../../../shared/lib/fuzzy";

/**
 * Saved prompts: reusable text (keywords, phrases, whole prompts) inserted
 * into any composer by typing `!`. Global, stored in the app database.
 */
export type SavedPrompt = {
  id: string;
  title: string;
  body: string;
  pinned: boolean;
  useCount: number;
  lastUsedAt?: number;
  createdAt: number;
  updatedAt: number;
};

export type SavedPromptUpsert = {
  id: string;
  title: string;
  body: string;
  pinned: boolean;
};

/** Emitted by the backend to every window after a save or delete. */
export const PROMPTS_CHANGED_EVENT = "monocode:prompts-changed";
/** Asks the app to open the Save prompt dialog with `{ text }`. */
export const SAVE_PROMPT_REQUEST_EVENT = "monocode:save-prompt-request";
/** The character that opens the saved-prompt picker in a composer. */
export const PROMPT_TRIGGER = "!";

const TITLE_MAX = 200;
export const PROMPT_BODY_MAX = 100_000;

export async function listSavedPrompts(): Promise<SavedPrompt[]> {
  const prompts = await invoke<SavedPrompt[] | null>("prompts_list");
  return Array.isArray(prompts) ? prompts : [];
}

export function saveSavedPrompt(
  prompt: SavedPromptUpsert,
): Promise<SavedPrompt> {
  return invoke<SavedPrompt>("prompts_upsert", { prompt });
}

export function deleteSavedPrompt(id: string): Promise<void> {
  return invoke<void>("prompts_delete", { id });
}

/** Counts an insert, so often-used prompts rise. Failures are not user-facing. */
export function markSavedPromptUsed(id: string): void {
  void invoke("prompts_mark_used", { id }).catch((error: unknown) =>
    console.warn("Could not record prompt use:", error),
  );
}

/** Re-runs `onChange` whenever any window saves or deletes a prompt. */
export function subscribeSavedPrompts(onChange: () => void): () => void {
  let disposed = false;
  let stop: (() => void) | undefined;
  // Outside Tauri (tests, browser preview) there is no event bridge.
  Promise.resolve()
    .then(() => listen(PROMPTS_CHANGED_EVENT, onChange))
    .then((unlisten) => {
      if (typeof unlisten !== "function") return;
      if (disposed) unlisten();
      else stop = unlisten;
    })
    .catch(() => undefined);
  return () => {
    disposed = true;
    stop?.();
  };
}

/** Opens the Save prompt dialog (mounted once in the app) with `text`. */
export function requestSavePrompt(text: string): void {
  window.dispatchEvent(
    new CustomEvent<{ text: string }>(SAVE_PROMPT_REQUEST_EVENT, {
      detail: { text },
    }),
  );
}

/** A title from the text's first line, trimmed to a readable length. */
export function promptTitleFrom(text: string, max = 60): string {
  const line =
    text
      .split(/\r?\n/)
      .map((part) => part.trim())
      .find(Boolean) ?? "";
  const plain = line.replace(/^#+\s*/, "").replace(/\s+/g, " ");
  if (plain.length <= max) return plain;
  const cut = plain.slice(0, max);
  const space = cut.lastIndexOf(" ");
  return `${(space > max / 2 ? cut.slice(0, space) : cut).trimEnd()}…`;
}

/** Validation shared by the Settings page and the Save prompt dialog. */
export function savedPromptError(title: string, body: string): string | null {
  if (!body.trim()) return "Add the text to insert.";
  if (body.length > PROMPT_BODY_MAX) return "This prompt is too long.";
  if (title.trim().length > TITLE_MAX) return "The title is too long.";
  return null;
}

/** What a prompt is called in lists: its title, else its first line. */
export function savedPromptLabel(
  prompt: Pick<SavedPrompt, "title" | "body">,
): string {
  return prompt.title.trim() || promptTitleFrom(prompt.body) || "Untitled";
}

/**
 * Prompts matching `query`, best first. With no query the stored order
 * (pinned, then most used) is kept. Titles weigh more than body text.
 */
export function rankSavedPrompts(
  prompts: readonly SavedPrompt[],
  query: string,
  limit = 8,
): SavedPrompt[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return prompts.slice(0, limit);
  const scored: { prompt: SavedPrompt; score: number; index: number }[] = [];
  prompts.forEach((prompt, index) => {
    const label = savedPromptLabel(prompt).toLowerCase();
    let score: number | null = null;
    if (label.startsWith(needle)) score = 3;
    else if (label.includes(needle)) score = 2;
    else if (fuzzyMatch(needle, label)) score = 1;
    else if (prompt.body.toLowerCase().includes(needle)) score = 0.5;
    if (score !== null) scored.push({ prompt, score, index });
  });
  return scored
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .slice(0, limit)
    .map((entry) => entry.prompt);
}

/** The `!query` being typed at the cursor. */
export type PromptToken = { start: number; end: number; query: string };

/**
 * `!` starts a token only at the start of the text or after whitespace, so
 * `Done!` and `a != b` never open the picker. The token runs to the next
 * whitespace; the menu closes again once nothing matches.
 */
export function promptTokenAt(
  text: string,
  cursor: number,
): PromptToken | null {
  const at = Math.max(0, Math.min(cursor, text.length));
  let start = at;
  while (start > 0 && !/\s/.test(text[start - 1]!)) start -= 1;
  if (text[start] !== PROMPT_TRIGGER || start === at) return null;
  let end = start + 1;
  while (end < text.length && !/\s/.test(text[end]!)) end += 1;
  const query = text.slice(start + 1, at);
  if (query.includes(PROMPT_TRIGGER)) return null;
  return { start, end, query };
}

/** Replaces the `!query` token with the prompt text; returns the new caret. */
export function insertSavedPrompt(
  text: string,
  token: PromptToken,
  body: string,
): { text: string; cursor: number } {
  const before = text.slice(0, token.start);
  const after = text.slice(token.end);
  const spacer = after && !/^\s/.test(after) ? " " : "";
  return {
    text: `${before}${body}${spacer}${after}`,
    cursor: before.length + body.length,
  };
}
