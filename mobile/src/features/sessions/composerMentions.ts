import { prepareLiteralMentions } from "./legacyMentions";
import { mentionTokenAt } from "./mentionToken";

/** A real blank glyph reserves one icon slot; Android never receives an @ to redraw. */
export const TAG_ICON_SLOT = "\u2003";

export type ComposerMention = {
  label: string;
  canonicalLabel: string;
  start: number;
  end: number;
  literalPath?: boolean;
  path: string;
  name: string;
  isDir: boolean;
};
export type MentionPart = {
  text: string;
  file?: ComposerMention;
  start: number;
};
const PUNCTUATION = new Set([
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

function validMention(text: string, file: ComposerMention) {
  const marker = text[file.start];
  if (
    (marker !== TAG_ICON_SLOT && marker !== "@") ||
    text.slice(file.start + 1, file.end) !== file.label
  )
    return false;
  const lineStart = text.lastIndexOf("\n", Math.max(0, file.start - 1)) + 1;
  if (/^ {0,3}>/.test(text.slice(lineStart, file.start))) return false;
  if (marker === "@" && !mentionTokenAt(text, file.start + 1)) return false;
  const tail = text.slice(file.end).match(/^\S*/)?.[0] ?? "";
  return [...tail].every((char) => PUNCTUATION.has(char));
}

/** References are ranges, so identical basenames can still name different files. */
export function composerMentionParts(
  text: string,
  files: ComposerMention[],
): MentionPart[] {
  const parts: MentionPart[] = [];
  let cursor = 0;
  for (const file of [...files].sort((a, b) => a.start - b.start)) {
    if (file.start < cursor || !validMention(text, file)) continue;
    if (file.start > cursor)
      parts.push({ text: text.slice(cursor, file.start), start: cursor });
    parts.push({
      text: text.slice(file.start, file.end),
      start: file.start,
      file,
    });
    cursor = file.end;
  }
  if (cursor < text.length)
    parts.push({ text: text.slice(cursor), start: cursor });
  return parts;
}

/** Shift references for edits outside a tag and untag edits inside one. */
export function updateComposerMentions(
  before: string,
  after: string,
  files: ComposerMention[],
  selection?: { start: number; end: number },
): ComposerMention[] {
  if (!files.length || before === after) return files;
  let start = 0;
  while (
    start < before.length &&
    start < after.length &&
    before[start] === after[start]
  )
    start++;
  let suffix = 0;
  while (
    suffix < before.length - start &&
    suffix < after.length - start &&
    before[before.length - suffix - 1] === after[after.length - suffix - 1]
  )
    suffix++;
  let end = before.length - suffix;
  if (selection) {
    // Text alone cannot tell which of two identical tags was deleted. Prefer
    // the native edit range when it explains the resulting string.
    const removed = before.length - after.length;
    const from =
      selection.start === selection.end && removed > 0
        ? Math.max(0, selection.start - removed)
        : selection.start;
    const to = selection.end;
    const inserted = after.length - (before.length - (to - from));
    if (
      inserted >= 0 &&
      after.slice(0, from) === before.slice(0, from) &&
      after.slice(from + inserted) === before.slice(to)
    ) {
      start = from;
      end = to;
    }
  }
  const delta = after.length - before.length;
  return files.flatMap((file) => {
    const moved =
      end <= file.start
        ? { ...file, start: file.start + delta, end: file.end + delta }
        : start >= file.end
          ? file
          : null;
    return moved && validMention(after, moved) ? [moved] : [];
  });
}

/** The visible basename is never used to guess a filesystem path on send. */
export function prepareComposerMentions(
  text: string,
  files: ComposerMention[],
) {
  return composerMentionParts(text, files)
    .map((part) => {
      if (!part.file) return part.text;
      const file = part.file;
      const canonical = `@${file.canonicalLabel}`;
      return file.literalPath
        ? prepareLiteralMentions(canonical, [
            { label: file.canonicalLabel, path: file.path, isDir: file.isDir },
          ])
        : canonical;
    })
    .join("");
}

export type MentionLine = {
  text: string;
  x: number;
  y: number;
  height: number;
};

/** Measure the prefix ending at the icon slot on its wrapped native text line.
 * The icon replaces the glyph visually, without changing its advance width. */
export function mentionIconPrefixes(
  text: string,
  parts: MentionPart[],
  lines: MentionLine[],
) {
  const out: {
    prefix: string;
    x: number;
    y: number;
    height: number;
    start: number;
    file: ComposerMention;
  }[] = [];
  let offset = 0;
  for (const line of lines) {
    const start = text.indexOf(line.text, offset);
    if (start < 0) continue;
    const end = start + line.text.length;
    for (const part of parts) {
      if (part.file && part.start >= start && part.start < end)
        out.push({
          prefix: text.slice(start, part.start + 1),
          x: line.x,
          y: line.y,
          height: line.height,
          start: part.start,
          file: part.file,
        });
    }
    offset = end;
  }
  return out;
}
