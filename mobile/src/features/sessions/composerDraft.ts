/**
 * How the phone's composer follows the text left in a thread's composer on
 * the desktop, which holds the one copy. Each desktop change has a revision;
 * the phone writes against the revision its text was based on, so a write
 * made offline can't wipe newer desktop text. Only the text travels; files
 * stay on each screen. See docs/native/composer-drafts-sync.md.
 */

import { useSyncExternalStore } from "react";

/** The desktop's copy, as `composer.read` answers. A cleared draft is revision 0. */
export type DesktopText = { text: string; rev: number };
/** What `composer.write` answers: whether it was kept, and the desktop's copy after. */
export type WriteReply = DesktopText & { accepted: boolean };
export type Write = { text: string; base: number; epoch: number };

/** The phone writes this long after the last keystroke. */
export const WRITE_DELAY_MS = 700;
/** For this long after a keystroke the person is typing here, and the
 *  desktop's text waits rather than replacing theirs. */
export const HOLD_MS = 3000;
/** How long a list row trusts the text here over the desktop's word, which
 *  trails a write by up to a poll and a publish. */
export const MARK_HOLD_MS = 10_000;

export type ComposerSync = {
  /** What the field shows. */
  text: string;
  /** The desktop revision `text` is based on; null until the first read. */
  base: number | null;
  /** The desktop's text at `base`, as far as the phone knows. */
  remote: string;
  /** The newest revision heard of, for the next read's `since`. */
  seen: number | null;
  /** A newer desktop copy, held while the person types here. */
  incoming: DesktopText | null;
  /** When the person last changed the text here; 0 if they haven't. */
  typedAt: number;
  /** When a change here last happened or reached the desktop. */
  markedAt: number;
  /** Writes sent and not answered yet. */
  writing: number;
  /** Moves on with each send, so answers to writes from before it are ignored. */
  epoch: number;
};

export const freshSync = (): ComposerSync => ({
  text: "",
  base: null,
  remote: "",
  seen: null,
  incoming: null,
  typedAt: 0,
  markedAt: 0,
  writing: 0,
  epoch: 0,
});

/** The text here has a change the desktop hasn't kept yet. */
export const unsent = (sync: ComposerSync) => sync.base !== null && sync.text !== sync.remote;

const typing = (sync: ComposerSync, now: number) => now - sync.typedAt < HOLD_MS;
const holding = (sync: ComposerSync, now: number) =>
  typing(sync, now) || sync.writing > 0 || unsent(sync);

/** Takes the desktop's copy. Returns the field's new text, or null if it is unchanged. */
function adopt(sync: ComposerSync, desktop: DesktopText): string | null {
  sync.base = desktop.rev;
  sync.remote = desktop.text;
  sync.incoming = null;
  if (sync.text === desktop.text) return null;
  sync.text = desktop.text;
  return desktop.text;
}

/** The person changed the text here. */
export function typed(sync: ComposerSync, text: string, now: number) {
  sync.text = text;
  sync.typedAt = now;
  sync.markedAt = now;
}

/**
 * A read answered. Returns the field's new text, or null to leave it: the
 * desktop's copy waits while the person is typing here or has a change on
 * its way, and is taken by `settle` once they stop.
 */
export function received(sync: ComposerSync, desktop: DesktopText, now: number): string | null {
  sync.seen = desktop.rev;
  if (sync.base === null) {
    // Typed before the first read came back: theirs is the newer text.
    if (sync.typedAt !== 0 && sync.text !== desktop.text) {
      sync.base = desktop.rev;
      sync.remote = desktop.text;
      return null;
    }
    return adopt(sync, desktop);
  }
  if (desktop.rev === sync.base) {
    sync.incoming = null;
    return null;
  }
  if (holding(sync, now)) {
    sync.incoming = desktop;
    return null;
  }
  return adopt(sync, desktop);
}

/** The person stopped typing: a desktop copy that waited is taken now. */
export function settle(sync: ComposerSync, now: number): string | null {
  if (!sync.incoming || holding(sync, now)) return null;
  return adopt(sync, sync.incoming);
}

/** The write to send now, if any; one at a time. */
export function startWrite(sync: ComposerSync): Write | null {
  if (sync.base === null || sync.writing > 0 || !unsent(sync)) return null;
  sync.writing += 1;
  return { text: sync.text, base: sync.base, epoch: sync.epoch };
}

/** A write failed to reach the desktop; it is tried again later. */
export function writeFailed(sync: ComposerSync) {
  sync.writing = Math.max(0, sync.writing - 1);
}

/**
 * A write answered. Refused means the desktop's text changed since this
 * one's base. If the person is typing here they are the one writing now, so
 * their text stays and goes again on the new base. Otherwise the write is an
 * old one, sent once the link came back, and the desktop's newer text wins.
 * Returns the field's new text, or null to leave it.
 */
export function written(
  sync: ComposerSync,
  write: Write,
  reply: WriteReply,
  now: number,
): string | null {
  sync.writing = Math.max(0, sync.writing - 1);
  sync.seen = reply.rev;
  sync.markedAt = now;
  // A send cleared the field since; its own answer says what's left.
  if (write.epoch !== sync.epoch) return null;
  if (reply.accepted) {
    sync.base = reply.rev;
    // What was sent, not the reply's: the desktop drops blank text, and
    // the field keeps it.
    sync.remote = write.text;
    sync.incoming = null;
    return null;
  }
  if (typing(sync, now)) {
    sync.base = reply.rev;
    sync.remote = reply.text;
    sync.incoming = null;
    return null;
  }
  return adopt(sync, reply);
}

/**
 * The message went from here. The field is empty, and the desktop clears
 * its copy if it is still the one the message came from. Returns that
 * revision for the send to carry.
 */
export function sent(sync: ComposerSync, now = Date.now()): number | undefined {
  const base = sync.base ?? undefined;
  sync.text = "";
  sync.remote = "";
  sync.incoming = null;
  sync.typedAt = 0;
  sync.markedAt = now;
  sync.epoch += 1;
  return base;
}

/** Each thread's composer while the app runs, so leaving a thread and
 *  coming back keeps an unsent change and shows the text at once. */
const threads = new Map<string, ComposerSync>();
const MAX_THREADS = 50;

export function syncFor(conversationId: string): ComposerSync {
  let sync = threads.get(conversationId);
  if (sync) {
    // Most recent last, so the oldest is the one let go.
    threads.delete(conversationId);
  } else {
    sync = freshSync();
    const oldest = threads.keys().next().value;
    if (threads.size >= MAX_THREADS && oldest !== undefined) threads.delete(oldest);
  }
  threads.set(conversationId, sync);
  return sync;
}

/** What a thread's composer opens with. */
export const cachedText = (conversationId?: string) =>
  (conversationId && threads.get(conversationId)?.text) || "";

/**
 * Whether the text here earns the thread a Draft mark. Null leaves it to the
 * desktop: once the text has settled there, the desktop also knows what
 * changed while this thread wasn't followed.
 */
export function localMark(sync: ComposerSync, now: number): boolean | null {
  const settled = sync.text === sync.remote && sync.writing === 0;
  return settled && now - sync.markedAt >= MARK_HOLD_MS ? null : sync.text.trim().length > 0;
}

const watchers = new Set<() => void>();
let settledTimer: ReturnType<typeof setTimeout> | undefined;
const tellWatchers = () => watchers.forEach((watcher) => watcher());

/** The text behind a Draft mark changed; rows look again now and once the hold ends. */
export function marksChanged() {
  tellWatchers();
  clearTimeout(settledTimer);
  settledTimer = setTimeout(tellWatchers, MARK_HOLD_MS);
}

function watchMarks(watcher: () => void) {
  watchers.add(watcher);
  return () => {
    watchers.delete(watcher);
  };
}

/** A row's Draft mark: the text here while it's newer, otherwise the desktop's word. */
export function useDraftMark(session: { id: string; draft?: boolean }) {
  const local = useSyncExternalStore(watchMarks, () => {
    const sync = threads.get(session.id);
    return sync ? localMark(sync, Date.now()) : null;
  });
  return local ?? !!session.draft;
}
