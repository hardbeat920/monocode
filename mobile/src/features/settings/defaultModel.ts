import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";

import { useConnection } from "@/features/connection/ConnectionProvider";
import type { ModelPick, RuntimeMode } from "@/features/models/types";
import type { Harness } from "@/features/workspace/types";

/** A model picked in Providers, with the name it is shown by. */
export type SavedModel = { pick: ModelPick; name: string };

/**
 * New sessions' model on this phone, after the desktop's providers settings:
 * the provider they start with, and each provider's model, remembered as it is
 * picked so switching back finds it. No provider leaves it to the desktop.
 */
export type DefaultModels = {
  harness: Harness | null;
  models: Partial<Record<Harness, SavedModel>>;
  /** The permission mode they start with; null leaves it to the desktop. */
  runtimeMode: RuntimeMode | null;
};

const EMPTY: DefaultModels = { harness: null, models: {}, runtimeMode: null };

// Per desktop, as each has its own providers and models.
const storageKey = (desktop: string) => `defaultModel:${desktop}`;

const cache = new Map<string, DefaultModels>();
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

function load(desktop: string): DefaultModels {
  const cached = cache.get(desktop);
  if (cached) return cached;
  let loaded = EMPTY;
  try {
    const raw = Storage.getItemSync(storageKey(desktop));
    if (raw) loaded = { ...EMPTY, ...JSON.parse(raw) };
  } catch {
    // Unreadable: as if never set.
  }
  cache.set(desktop, loaded);
  return loaded;
}

function save(desktop: string, next: DefaultModels) {
  cache.set(desktop, next);
  void Storage.setItem(storageKey(desktop), JSON.stringify(next));
  listeners.forEach((listener) => listener());
}

/** The paired desktop's defaults, and setters that save them. */
export function useDefaultModels() {
  const desktop = useConnection().pairing?.desktop.deviceId ?? "";
  const defaults = useSyncExternalStore(subscribe, () => load(desktop));
  return {
    defaults,
    /** Starts new sessions on `harness`, with `model` if given or its last. */
    setProvider: (harness: Harness | null, model?: SavedModel) =>
      save(desktop, {
        ...defaults,
        harness,
        models: model
          ? { ...defaults.models, [model.pick.harness]: model }
          : defaults.models,
      }),
    /** Remembers `model` for its provider, and starts new sessions on it. */
    setModel: (model: SavedModel) =>
      save(desktop, {
        ...defaults,
        harness: model.pick.harness,
        models: { ...defaults.models, [model.pick.harness]: model },
      }),
    /** Starts new sessions with `runtimeMode`, or the desktop's with null. */
    setRuntimeMode: (runtimeMode: RuntimeMode | null) =>
      save(desktop, { ...defaults, runtimeMode }),
  };
}

/** The model new sessions start with on this phone, or null for the desktop's. */
export function useDefaultModel(): SavedModel | null {
  const { defaults } = useDefaultModels();
  return defaults.harness ? (defaults.models[defaults.harness] ?? null) : null;
}

/** The permission mode new sessions start with on this phone, or null for the desktop's. */
export function useDefaultRuntimeMode(): RuntimeMode | null {
  return useDefaultModels().defaults.runtimeMode;
}
