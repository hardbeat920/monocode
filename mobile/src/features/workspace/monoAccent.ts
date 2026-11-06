import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";

import { ACCENTS } from "@/ui/theme";

/** An accent picked for one Mono's chat, or "theme" for the app's own. */
export type MonoAccent = keyof typeof ACCENTS | "theme";

// Kept on this phone, per desktop and Mono, as reactions are.
const KEY = "monocode.monoAccents";
let accents: Readonly<Record<string, MonoAccent>> = load();
const listeners = new Set<() => void>();

function load() {
  try {
    return JSON.parse(Storage.getItemSync(KEY) ?? "{}") as Record<string, MonoAccent>;
  } catch {
    return {};
  }
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Draws this Mono's chat in `accent`, or back in the app's own for "theme". */
export function setMonoAccent(monoId: string, accent: MonoAccent) {
  const { [monoId]: _, ...rest } = accents;
  accents = accent === "theme" ? rest : { ...rest, [monoId]: accent };
  void Storage.setItem(KEY, JSON.stringify(accents));
  listeners.forEach((listener) => listener());
}

/** The accent picked for this Mono's chat. */
export function useMonoAccent(monoId: string | undefined): MonoAccent {
  return useSyncExternalStore(subscribe, () => (monoId ? accents[monoId] : undefined) ?? "theme");
}
