import Storage from "expo-sqlite/kv-store";
import { useEffect, useMemo, useSyncExternalStore } from "react";

import type { Mono } from "./types";

// What the user last saw of each mono, per desktop: its activity then, so a
// reply that came after reads as new. Kept on the phone, as the desktop sends
// no count of its own.
type Seen = Readonly<Record<string, string>>;

const storageKey = (desktop: string) => `monocode.monoSeen.${desktop}`;
let loadedFor: string | undefined;
let seen: Seen = {};
const listeners = new Set<() => void>();

function load(desktop: string | undefined) {
  if (!desktop || desktop === loadedFor) return;
  loadedFor = desktop;
  try {
    seen = JSON.parse(Storage.getItemSync(storageKey(desktop)) ?? "{}") as Seen;
  } catch {
    seen = {};
  }
}

function save(next: Seen) {
  seen = next;
  if (loadedFor) void Storage.setItem(storageKey(loadedFor), JSON.stringify(next));
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** A mono's last reply, as far as the phone can tell; null when it can't,
 *  as when the desktop sends no message for a conversation it hasn't open. */
function activity(mono: Mono) {
  return mono.lastMessage || mono.lastActiveAt
    ? `${mono.lastActiveAt}\n${mono.lastMessage}`
    : null;
}

/** Marks the mono's latest as seen, as its chat shows it. */
export function markMonoSeen(desktop: string | undefined, mono: Mono) {
  load(desktop);
  const now = activity(mono);
  if (now !== null && seen[mono.id] !== now) save({ ...seen, [mono.id]: now });
}

/** Marks the mono's latest as not seen, so it reads as new until opened again. */
export function markMonoUnread(desktop: string | undefined, mono: Mono) {
  load(desktop);
  if (seen[mono.id] !== "") save({ ...seen, [mono.id]: "" });
}

/**
 * The monos with a reply the user hasn't seen. One seen for the first time
 * counts as seen, so a new phone doesn't open to every mono marked; one still
 * working has nothing finished to show yet.
 */
export function useUnreadMonos(desktop: string | undefined, monos: Mono[]): Set<string> {
  load(desktop);
  const current = useSyncExternalStore(subscribe, () => seen);
  useEffect(() => {
    const fresh = monos.filter((mono) => !(mono.id in seen) && activity(mono) !== null);
    if (fresh.length)
      save({ ...seen, ...Object.fromEntries(fresh.map((mono) => [mono.id, activity(mono)!])) });
  }, [monos]);
  return useMemo(
    () =>
      new Set(
        monos
          .filter((mono) => {
            if (mono.status === "working") return false;
            if (mono.unread > 0) return true;
            const now = activity(mono);
            return now !== null && mono.id in current && current[mono.id] !== now;
          })
          .map((mono) => mono.id),
      ),
    [monos, current],
  );
}
