/**
 * Composer prompt history, shared across sessions.
 *
 * Claude and Codex CLIs let you walk previously sent prompts with the arrow
 * keys. MonoCode keeps the same list in memory (and localStorage when it is
 * available) so a prompt sent in one session can be recalled in another.
 */

export const COMPOSER_HISTORY_LIMIT = 100;
const COMPOSER_HISTORY_STORAGE_KEY = "monocode.composerHistory.v1";

export type ComposerHistoryDirection = "up" | "down";

export type ComposerHistoryCursor = {
  /** Index into the stored list, or `null` when showing the live draft. */
  index: number | null;
  stash: string;
};

export const EMPTY_COMPOSER_HISTORY_CURSOR: ComposerHistoryCursor = {
  index: null,
  stash: "",
};

export type ComposerHistoryStep = {
  cursor: ComposerHistoryCursor;
  text: string;
};

type HistoryKeyEvent = {
  key: string;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  selectionStart: number;
  selectionEnd: number;
  value: string;
};

let memory: string[] | null = null;

export function parseComposerHistory(raw: string): string[] {
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (entry): entry is string =>
        typeof entry === "string" && entry.trim() !== "",
    );
  } catch {
    return [];
  }
}

function readStored(): string[] {
  try {
    if (typeof localStorage === "undefined") return [];
    const raw = localStorage.getItem(COMPOSER_HISTORY_STORAGE_KEY);
    return raw ? parseComposerHistory(raw) : [];
  } catch {
    return [];
  }
}

function writeStored(entries: readonly string[]) {
  try {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(COMPOSER_HISTORY_STORAGE_KEY, JSON.stringify(entries));
  } catch {
    // private mode / quota
  }
}

function stored(): string[] {
  if (!memory) memory = readStored();
  return memory;
}

export function loadComposerHistory(): string[] {
  return stored().slice();
}

export function clearComposerHistory() {
  memory = [];
  writeStored([]);
}

function normalizeComposerHistoryText(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

export function pushComposerHistory(
  entries: readonly string[],
  text: string,
  limit = COMPOSER_HISTORY_LIMIT,
): string[] {
  const value = normalizeComposerHistoryText(text);
  if (!value.trim()) return entries.slice();
  if (entries[entries.length - 1] === value) return entries.slice();
  const next = [...entries, value];
  return next.length > limit ? next.slice(next.length - limit) : next;
}

export function recordComposerHistory(text: string): void {
  const next = pushComposerHistory(stored(), text);
  memory = next;
  writeStored(next);
}

function isComposerHistoryUpPosition(text: string, cursor: number): boolean {
  return !text.slice(0, cursor).includes("\n");
}

function isComposerHistoryDownPosition(text: string, cursor: number): boolean {
  return !text.slice(cursor).includes("\n");
}

export function composerHistoryDirection(
  event: HistoryKeyEvent,
): ComposerHistoryDirection | null {
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
    return null;
  }
  if (event.selectionStart !== event.selectionEnd) return null;
  if (
    event.key === "ArrowUp" &&
    isComposerHistoryUpPosition(event.value, event.selectionStart)
  ) {
    return "up";
  }
  if (
    event.key === "ArrowDown" &&
    isComposerHistoryDownPosition(event.value, event.selectionStart)
  ) {
    return "down";
  }
  return null;
}

export function stepComposerHistory(
  entries: readonly string[],
  cursor: ComposerHistoryCursor,
  direction: ComposerHistoryDirection,
  currentText: string,
): ComposerHistoryStep {
  if (entries.length === 0) {
    return { cursor, text: currentText };
  }

  if (direction === "up") {
    if (cursor.index === null) {
      const index = entries.length - 1;
      return {
        cursor: { index, stash: currentText },
        text: entries[index] ?? currentText,
      };
    }
    if (cursor.index <= 0) return { cursor, text: currentText };
    const index = cursor.index - 1;
    return {
      cursor: { ...cursor, index },
      text: entries[index] ?? currentText,
    };
  }

  if (cursor.index === null) return { cursor, text: currentText };
  if (cursor.index >= entries.length - 1) {
    return {
      cursor: EMPTY_COMPOSER_HISTORY_CURSOR,
      text: cursor.stash,
    };
  }
  const index = cursor.index + 1;
  return { cursor: { ...cursor, index }, text: entries[index] ?? currentText };
}
