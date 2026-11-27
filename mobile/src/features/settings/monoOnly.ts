import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";

const STORAGE_KEY = "layout.monoOnly";

function load(): boolean {
  try {
    return Storage.getItemSync(STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

let monoOnly: boolean | null = null;
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

/**
 * Whether the phone shows only Monos, as a messaging app: a list of them,
 * and a chat pushed over it, with no drawer and no projects. Off until
 * turned on.
 */
export function useMonoOnlyLayout() {
  const value = useSyncExternalStore(subscribe, () => (monoOnly ??= load()));
  const set = (next: boolean) => {
    monoOnly = next;
    void Storage.setItem(STORAGE_KEY, String(next));
    listeners.forEach((listener) => listener());
  };
  return [value, set] as const;
}

/** How long the settings sheet takes to slide away, with the cover coming
 *  up under it, before the app is reset beneath the cover. */
const DISMISS_MS = 500;
/** How long the cover stays up once the new layout is in, for its first
 *  screen to draw before it shows. */
const SETTLE_MS = 400;

let changing = false;
const changeListeners = new Set<() => void>();
const subscribeChange = (listener: () => void) => {
  changeListeners.add(listener);
  return () => {
    changeListeners.delete(listener);
  };
};
const setChanging = (next: boolean) => {
  changing = next;
  changeListeners.forEach((listener) => listener());
};

/** Whether the layout is being switched, for the app to cover itself meanwhile. */
export function useLayoutChanging() {
  return useSyncExternalStore(subscribeChange, () => changing);
}

/**
 * Switches the layout, in order, so nothing half-built ever shows: `close`
 * slides the settings sheet away over the loading cover, which comes up
 * under it; once it is gone, `reset` takes the app back to its first screen
 * and the layout changes, all beneath the cover; and the cover plays its way
 * out once the new first screen (the Mono list, or the last Mono or project)
 * has had a moment to draw. The two lay the app out too differently to carry
 * over what was open.
 */
export function useChangeLayout() {
  const [, set] = useMonoOnlyLayout();
  return (next: boolean, { close, reset }: { close: () => void; reset: () => void }) => {
    if (changing) return;
    close();
    setChanging(true);
    setTimeout(() => {
      reset();
      set(next);
      setTimeout(() => setChanging(false), SETTLE_MS);
    }, DISMISS_MS);
  };
}
