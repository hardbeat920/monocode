export type MarkdownDocumentParts = {
  metadata: string | null;
  body: string;
};

/** Split a leading YAML frontmatter block without attempting to interpret it. */
export function splitMarkdownFrontmatter(text: string): MarkdownDocumentParts {
  const opening = text.match(/^\uFEFF?---[ \t]*\r?\n/);
  if (!opening) return { metadata: null, body: text };

  const remaining = text.slice(opening[0].length);
  const closing = /^(?:---|\.\.\.)[ \t]*(?:\r?\n|$)/m.exec(remaining);
  // An unfinished header remains visible as ordinary document text.
  if (!closing) return { metadata: null, body: text };

  return {
    metadata: remaining.slice(0, closing.index).replace(/\r?\n$/, ""),
    body: remaining.slice(closing.index + closing[0].length),
  };
}
