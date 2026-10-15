/** Where in a document a passage was cited. */
export type ChatReferenceSource = {
  /** Shown to the user and sent to the agent, e.g. `@src/app.ts (lines 3-5)`. */
  label: string;
  /** Workspace-relative path, matching the open editor tab. */
  path: string;
  /** Offsets of the cited text when it was cited; the editor tracks edits itself. */
  from: number;
  to: number;
};

/** A piece of the transcript or a document the user cited, with their note on it. */
export type ChatReference = {
  id: string;
  text: string;
  note: string;
  /** Where it was cited in the transcript, so that spot can be marked and clicked. */
  range?: Range;
  /** Set when the passage came from a document rather than the transcript. */
  source?: ChatReferenceSource;
};

/** A new reference with normalised line endings and trimmed text and note. */
export function newChatReference(
  text: string,
  note: string,
  range?: Range,
  source?: ChatReferenceSource,
): ChatReference {
  return {
    id: crypto.randomUUID(),
    text: text.replace(/\r\n?/g, "\n").trim(),
    note: note.replace(/\r\n?/g, "\n").trim(),
    range,
    source,
  };
}

/** The list with one reference's note replaced. */
export function updateChatReferenceNote(
  references: ChatReference[],
  id: string,
  note: string,
): ChatReference[] {
  const next = note.replace(/\r\n?/g, "\n").trim();
  return references.map((reference) =>
    reference.id === id ? { ...reference, note: next } : reference,
  );
}

/** The list without one reference. */
export function removeChatReference(
  references: ChatReference[],
  id: string,
): ChatReference[] {
  return references.filter((reference) => reference.id !== id);
}

/** The pill's label, e.g. "1 annotation" or "3 annotations". */
export function chatReferencesLabel(count: number): string {
  return `${count} ${count === 1 ? "annotation" : "annotations"}`;
}

/** The message the agent receives: the user's text, then each cited passage with its note. */
export function composeWithChatReferences(
  text: string,
  references: ChatReference[],
): string {
  if (references.length === 0) return text;
  const blocks = references.map((reference, index) => {
    const quote = reference.text
      .split("\n")
      .map((line) => (line ? `> ${line}` : ">"))
      .join("\n");
    return [
      `Reference ${index + 1}${reference.source ? ` — ${reference.source.label}` : ""}:`,
      quote,
      ...(reference.note ? ["", `Note: ${reference.note}`] : []),
    ].join("\n");
  });
  return [text.trimEnd(), ...blocks].filter(Boolean).join("\n\n");
}
