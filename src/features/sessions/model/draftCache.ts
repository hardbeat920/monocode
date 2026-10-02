/**
 * In-memory composer drafts, keyed by session id.
 *
 * SessionPane keeps the text you are typing in a React ref so retyping while
 * the pane stays mounted does not re-render on every keystroke. A plain ref
 * only lives as long as that one component instance though: closing a
 * session's pane (or moving it to another split) unmounts SessionPane, and
 * the ref - and whatever you had typed - is gone with no warning the moment
 * it remounts.
 *
 * This module-level map survives that, because it lives outside the React
 * tree for as long as the app process is running: the draft comes back when
 * the pane for that session opens again.
 *
 * It does not survive an app restart or a full reload - that needs the
 * draft written into the session's persisted record on disk, which is a
 * separate, larger change (touches the Rust-side session_upsert schema
 * too).
 */
import type { McpTag } from "./mcpPicker";

const drafts = new Map<string, string>();
const mcpTags = new Map<string, McpTag[]>();

export function getComposerDraft(sessionId: string): string | undefined {
  return drafts.get(sessionId);
}

export function setComposerDraft(sessionId: string, text: string): void {
  if (text) {
    drafts.set(sessionId, text);
  } else {
    drafts.delete(sessionId);
    mcpTags.delete(sessionId);
  }
}

export function getComposerMcpTags(sessionId: string): McpTag[] {
  return mcpTags.get(sessionId) ?? [];
}

export function setComposerMcpTags(sessionId: string, tags: McpTag[]): void {
  if (tags.length > 0) {
    mcpTags.set(sessionId, tags);
  } else {
    mcpTags.delete(sessionId);
  }
}

export function clearComposerDraft(sessionId: string): void {
  drafts.delete(sessionId);
  mcpTags.delete(sessionId);
}

// A session can have more than one mounted composer (shared panes or draft edits).
const closeGuards = new Map<string, Set<() => boolean>>();

export function registerComposerCloseGuard(
  sessionId: string,
  hasUnsavedWork: () => boolean,
): () => void {
  const guards = closeGuards.get(sessionId) ?? new Set();
  guards.add(hasUnsavedWork);
  closeGuards.set(sessionId, guards);
  return () => {
    guards.delete(hasUnsavedWork);
    if (guards.size === 0) closeGuards.delete(sessionId);
  };
}

export function hasUnsavedComposerDraft(sessionId: string): boolean {
  return (
    !!getComposerDraft(sessionId) ||
    [...(closeGuards.get(sessionId) ?? [])].some((guard) => guard())
  );
}
