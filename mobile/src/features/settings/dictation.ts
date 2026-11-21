import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";

import {
  DEFAULT_PREFERENCE,
  type DictationPreference,
  PARAKEET_MODEL_IDS,
  type EngineId,
  type ParakeetModelId,
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
      parakeetModel: PARAKEET_MODEL_IDS.includes(value.parakeetModel as ParakeetModelId)
        ? (value.parakeetModel as ParakeetModelId)
        : DEFAULT_PREFERENCE.parakeetModel,
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
  // Written one after another, in the order picked, so a slower earlier
  // write can't land last; each pick gets its own write's result.
  const value = JSON.stringify(preference);
  const write = writes.catch(() => {}).then(() => Storage.setItem(STORAGE_KEY, value));
  writes = write;
  return write;
}

let writes: Promise<void> = Promise.resolve();

export function useDictationPreference() {
  const value = useSyncExternalStore(subscribe, getDictationPreference);
  return [value, setDictationPreference] as const;
}
