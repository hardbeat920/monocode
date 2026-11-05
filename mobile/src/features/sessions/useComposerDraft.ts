import { useIsFocused } from "expo-router";
import { useEffect, useState } from "react";
import { AppState } from "react-native";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { backoff, pause, waitForActive } from "@/features/connection/appActivity";
import { makeCommandId } from "@/features/connection/nativeLink";

import {
  HOLD_MS,
  received,
  sent,
  settle,
  startWrite,
  marksChanged,
  syncFor,
  typed,
  unsent,
  writeFailed,
  written,
  WRITE_DELAY_MS,
  type DesktopText,
  type WriteReply,
} from "./composerDraft";

/** After a failed read or write, the phone tries again this much later. */
const RETRY_MS = 2000;

/** After repeated failures the phone waits longer, up to this. */
const RETRY_MAX_MS = 30000;

type Command = <T>(method: string, params?: object, id?: string) => Promise<T>;

export type SharedComposer = {
  /** The person changed the text. */
  typed: (text: string) => void;
  /** Writes an unsent change now, as the field lets go. */
  flush: () => void;
  /** The message went; returns the revision it was based on, for the send. */
  sent: () => number | undefined;
};
type Link = SharedComposer & {
  /** Follows the desktop's copy until the returned stop is called. */
  start: (command: Command, show: (text: string) => void) => () => void;
};

const NONE: SharedComposer = { typed: () => {}, flush: () => {}, sent: () => undefined };

/** One thread's composer and the desktop's copy, for as long as it is on screen. */
function link(conversationId: string): Link {
  const sync = syncFor(conversationId);
  let command: Command | null = null;
  let show: (text: string) => void = () => {};
  let writeTimer: ReturnType<typeof setTimeout> | null = null;
  let settleTimer: ReturnType<typeof setTimeout> | null = null;
  const apply = (text: string | null) => {
    if (text !== null) show(text);
  };
  const clearWrite = () => {
    if (writeTimer) clearTimeout(writeTimer);
    writeTimer = null;
  };
  const later = (ms: number) => {
    if (!unsent(sync) || writeTimer) return;
    writeTimer = setTimeout(write, ms);
  };
  function write() {
    clearWrite();
    const next = command && startWrite(sync);
    if (!command || !next) return;
    command<WriteReply>(
      "composer.write",
      { conversationId, text: next.text, base: next.base },
      makeCommandId(),
    ).then(
      (reply) => {
        apply(written(sync, next, reply, Date.now()));
        marksChanged();
        later(WRITE_DELAY_MS);
      },
      () => {
        // Offline, or the desktop said no: the change stays and is tried again.
        writeFailed(sync);
        later(RETRY_MS);
      },
    );
  }

  return {
    start(next, onShow) {
      command = next;
      show = onShow;
      let alive = true;
      const stop = new AbortController();
      void (async () => {
        let failures = 0;
        while (alive) {
          // Nothing to follow while the app is in the background; wait to be shown again.
          await waitForActive(stop.signal);
          if (!alive) return;
          try {
            // Waits on the desktop until its copy moves past the one seen.
            const desktop = await next<DesktopText>("composer.read", {
              conversationId,
              ...(sync.seen !== null ? { since: sync.seen } : {}),
            });
            if (!alive) return;
            failures = 0;
            apply(received(sync, desktop, Date.now()));
            // A change typed before the first read goes now it has a base.
            if (unsent(sync) && !writeTimer) write();
          } catch {
            if (!alive) return;
            await pause(backoff(RETRY_MS, failures++, RETRY_MAX_MS), stop.signal);
          }
        }
      })();
      // Going to the background writes what is left, while it still can.
      const appState = AppState.addEventListener("change", (state) => {
        if (state !== "active") write();
      });
      return () => {
        alive = false;
        stop.abort();
        appState.remove();
        if (settleTimer) clearTimeout(settleTimer);
        show = () => {};
        // Leaving the thread writes what is left; the cache keeps it if that fails.
        write();
      };
    },
    typed(text) {
      typed(sync, text, Date.now());
      marksChanged();
      clearWrite();
      writeTimer = setTimeout(write, WRITE_DELAY_MS);
      // A desktop copy that waited is taken once typing stops.
      if (settleTimer) clearTimeout(settleTimer);
      settleTimer = setTimeout(() => {
        settleTimer = null;
        apply(settle(sync, Date.now()));
      }, HOLD_MS);
    },
    flush: write,
    sent() {
      clearWrite();
      const base = sent(sync);
      marksChanged();
      return base;
    },
  };
}

/**
 * A thread's composer text, shared with the desktop's composer for it. Reads
 * follow the desktop's copy (each waits there until it changes); typing here
 * is written once it pauses. `show` puts the desktop's text in the field.
 * Without `conversationId`, a new session's or a Mono's composer, nothing
 * is shared.
 */
export function useComposerDraft(
  conversationId: string | undefined,
  show: (text: string) => void,
): SharedComposer {
  const { command } = useConnection();
  const [shared] = useState(() => (conversationId ? link(conversationId) : null));
  // A thread behind another screen isn't followed; it picks up where it left off.
  const focused = useIsFocused();
  useEffect(
    () => (focused ? shared?.start(command, show) : undefined),
    [shared, command, show, focused],
  );
  return shared ?? NONE;
}
