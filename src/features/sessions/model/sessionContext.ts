import {
  buildPortableContext,
  buildPortableContextSnapshot,
  historicalContextAttachments,
  portableContextManifest,
  type PortableContext,
} from "./portableContext";
import {
  snapshotContextAssets,
  type ContextAssetSnapshot,
} from "./contextAssets";
import { saveProviderContextSnapshot } from "../data/sessionStore";
import type { Session } from "./session";

/**
 * Another session added to the composer as context. Only the reference is
 * kept; the recap is read from the source session when the turn is sent.
 */
export type SessionContextCard = {
  id: string;
  title: string;
};

export type SessionDropChoice = "context" | "link";

/** Fired on `window` when a sidebar session is dropped on a composer. */
export const SESSION_COMPOSER_DROP_EVENT = "monocode:session-composer-drop";

export type SessionComposerDrop = {
  fromId: string;
  targetId: string;
  choice: SessionDropChoice;
};

export function requestSessionComposerDrop(drop: SessionComposerDrop) {
  if (typeof window === "undefined" || drop.fromId === drop.targetId) return;
  window.dispatchEvent(
    new CustomEvent<SessionComposerDrop>(SESSION_COMPOSER_DROP_EVENT, {
      detail: drop,
    }),
  );
}

export const SESSION_CONTEXT_LEAD = "Use the attached session as context.";

const MAX_CARDS = 8;

/** Add a card once. A session is never added to its own composer. */
export function withSessionContextCard(
  cards: SessionContextCard[] | undefined,
  card: SessionContextCard,
  targetId: string,
): SessionContextCard[] | undefined {
  if (card.id === targetId) return cards;
  const current = cards ?? [];
  if (current.some((entry) => entry.id === card.id)) return cards;
  if (current.length >= MAX_CARDS) return cards;
  return [...current, { id: card.id, title: card.title.trim() }];
}

export function withoutSessionContextCard(
  cards: SessionContextCard[] | undefined,
  id: string,
): SessionContextCard[] | undefined {
  const next = cards?.filter((card) => card.id !== id);
  return next?.length ? next : undefined;
}

function attribute(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/\s+/g, " ")
    .trim();
}

function unescapeAttribute(value: string): string {
  return value
    .replace(/&quot;/g, '"')
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&");
}

/** Keep a transcript from closing the block it is quoted in. */
function body(value: string): string {
  return value.replace(/<(\/?)(attached_context|session)\b/gi, "&lt;$1$2");
}

export type SessionContextEntry = SessionContextCard & {
  /** Null when the source session could not be read. */
  context: PortableContext | null;
};

/** Total history budget shared by every attached session, in bytes. */
export const SESSION_CONTEXT_MAX_BYTES = 16_000;
const MIN_SESSION_BYTES = 4_000;

/**
 * One session's history as the agent reads it. This is the manifest and item
 * JSON that `renderPortableContext` uses, without its "continue this
 * conversation" lead and current request, since this history belongs to a
 * different session.
 */
function renderSessionHistory(context: PortableContext | null): string {
  if (!context) return "(This session is no longer available.)";
  if (context.items.length === 0)
    return "(This session has no messages yet.)";
  return [
    portableContextManifest(context),
    JSON.stringify(context.items),
  ].join("\n");
}

const ATTACHED_CONTEXT_HEADER =
  "The user attached other MonoCode sessions for reference. They are separate conversations, not this one. Each holds a manifest and that session's user and assistant messages as JSON. Older messages may be left out for size; read the manifest's retrievalPath, when present, for the full transcript, including tool activity. Attachments are file references only.";

/** The block the agent reads; `parseAttachedContext` turns it back into cards. */
export function formatAttachedContext(entries: SessionContextEntry[]): string {
  if (entries.length === 0) return "";
  const sessions = entries.map(
    (entry) =>
      `<session id="${attribute(entry.id)}" title="${attribute(entry.title || "Untitled session")}">\n${body(renderSessionHistory(entry.context))}\n</session>`,
  );
  return [
    "<attached_context>",
    ATTACHED_CONTEXT_HEADER,
    ...sessions,
    "</attached_context>",
  ].join("\n");
}

export function appendAttachedContext(
  text: string,
  entries: SessionContextEntry[],
): string {
  const block = formatAttachedContext(entries);
  if (!block) return text;
  const lead = text.trim() || SESSION_CONTEXT_LEAD;
  return `${lead}\n\n${block}`;
}

const ATTACHED_CONTEXT_OPEN = `<attached_context>\n${ATTACHED_CONTEXT_HEADER}\n`;
const ATTACHED_CONTEXT_CLOSE = "</attached_context>";
// `body` escapes these tags inside transcripts, so a line-start tag is ours.
const SESSION_TAG = /^<session id="([^"]*)" title="([^"]*)">$/gm;

/** Split a sent prompt back into the user's words and the attached sessions. */
export function parseAttachedContext(text: string): {
  text: string;
  sessions: SessionContextCard[];
} {
  // Only the block that appendAttachedContext put at the end counts. Text the
  // user typed that looks like one stays in the message.
  const start = text.lastIndexOf(ATTACHED_CONTEXT_OPEN);
  const trimmed = text.trimEnd();
  if (start < 0 || !trimmed.endsWith(ATTACHED_CONTEXT_CLOSE))
    return { text, sessions: [] };
  const inner = trimmed.slice(
    start + ATTACHED_CONTEXT_OPEN.length,
    trimmed.length - ATTACHED_CONTEXT_CLOSE.length,
  );
  const sessions: SessionContextCard[] = [];
  for (const tag of inner.matchAll(SESSION_TAG)) {
    sessions.push({
      id: unescapeAttribute(tag[1]),
      title: unescapeAttribute(tag[2]),
    });
  }
  if (sessions.length === 0) return { text, sessions: [] };
  const visible = text.slice(0, start).trim();
  return {
    text: sessions.length && visible === SESSION_CONTEXT_LEAD ? "" : visible,
    sessions,
  };
}

export type SessionContextStorage = {
  snapshotAssets(
    sessionId: string,
    attachments: ReturnType<typeof historicalContextAttachments>,
  ): Promise<ContextAssetSnapshot[]>;
  saveSnapshot(
    sessionId: string,
    snapshotId: string,
    content: string,
  ): Promise<string>;
};

const defaultStorage: SessionContextStorage = {
  snapshotAssets: snapshotContextAssets,
  saveSnapshot: saveProviderContextSnapshot,
};

/**
 * One source session as portable context: user and assistant messages within
 * the byte budget, plus a saved snapshot of the whole transcript that the
 * target agent can read from `retrievalPath`.
 */
export async function sessionContextFor(
  source: Session,
  maxBytes: number,
  storage: SessionContextStorage = defaultStorage,
): Promise<PortableContext> {
  const throughBlockId = source.blocks[source.blocks.length - 1]?.id;
  const assets = await storage
    .snapshotAssets(
      source.id,
      historicalContextAttachments(source, throughBlockId),
    )
    .catch((): ContextAssetSnapshot[] => []);
  const prose: Session = {
    ...source,
    blocks: source.blocks.filter(
      (block) => block.role === "user" || block.role === "assistant",
    ),
  };
  const context = buildPortableContext(prose, {
    maxBytes,
    assetSnapshots: assets,
  });
  if (throughBlockId) {
    try {
      context.retrievalPath = await storage.saveSnapshot(
        source.id,
        crypto.randomUUID(),
        buildPortableContextSnapshot(source, throughBlockId, assets),
      );
    } catch {
      // The inline history still goes out; only the full copy is missing.
    }
  }
  return context;
}

/** Read each attached session now and append its history to the prompt. */
export async function expandSessionContext(
  text: string,
  cards: SessionContextCard[] | undefined,
  load: (id: string) => Promise<Session | null | undefined>,
  storage: SessionContextStorage = defaultStorage,
): Promise<string> {
  if (!cards?.length) return text;
  const maxBytes = Math.max(
    MIN_SESSION_BYTES,
    Math.floor(SESSION_CONTEXT_MAX_BYTES / cards.length),
  );
  const entries = await Promise.all(
    cards.map(async (card): Promise<SessionContextEntry> => {
      const source = await load(card.id).catch(() => null);
      return {
        id: card.id,
        title: source?.title || card.title,
        context: source
          ? await sessionContextFor(source, maxBytes, storage)
          : null,
      };
    }),
  );
  return appendAttachedContext(text, entries);
}
