import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";

import {
  DEFAULT_PREFERENCE,
  type DictationPreference,
  type EngineId,
} from "@/features/sessions/dictationLanguages";

const STORAGE_KEY = "dictation.preference";
const ENGINES: readonly EngineId[] = ["apple", "parakeet"];

/** A stored preference, or the default for anything missing or unreadable. */
export function parseDictationPreference(stored: string | null): DictationPreference {
  if (!stored) return DEFAULT_PREFERENCE;
  try {
    const value = JSON.parse(stored) as Partial<DictationPreference>;
    return {
      engine: ENGINES.includes(value.engine as EngineId)
        ? (value.engine as EngineId)
        : DEFAULT_PREFERENCE.engine,
      language:
        typeof value.language === "string" && value.language.length > 0
          ? value.language
          : null,
    };
  } catch {
    return DEFAULT_PREFERENCE;
  }
}

function load(): DictationPreference {
  try {
    return parseDictationPreference(Storage.getItemSync(STORAGE_KEY));
  } catch {
    return DEFAULT_PREFERENCE;
  }
}

// Held here, so a dictation started after a change in settings uses it.
let preference: DictationPreference | null = null;
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

/** The engine and language dictation uses, kept across launches. */
export function getDictationPreference() {
  return (preference ??= load());
}

/** Applies at once; the returned promise is the write to storage, which
 *  rejects if the pick won't outlast this launch. */
export function setDictationPreference(next: Partial<DictationPreference>): Promise<void> {
  preference = { ...getDictationPreference(), ...next };
  listeners.forEach((listener) => listener());
  return Storage.setItem(STORAGE_KEY, JSON.stringify(preference));
}

export function useDictationPreference() {
  const value = useSyncExternalStore(subscribe, getDictationPreference);
  return [value, setDictationPreference] as const;
}
