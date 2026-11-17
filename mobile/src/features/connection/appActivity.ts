import { AppState } from "react-native";

/** Whether the app is in the foreground. */
export const isAppActive = () => AppState.currentState === "active";

/**
 * Resolves once the app is in the foreground: at once when it already is,
 * otherwise on the next `active` event. A loop waits here instead of sleeping
 * and checking, so it costs nothing while the phone is locked or the app is
 * in the background. Aborting resolves it too, so the caller can stop.
 */
export function waitForActive(signal?: AbortSignal): Promise<void> {
  if (isAppActive() || signal?.aborted) return Promise.resolve();
  return new Promise((resolve) => {
    const done = () => {
      subscription.remove();
      signal?.removeEventListener("abort", done);
      resolve();
    };
    const subscription = AppState.addEventListener("change", (state) => {
      if (state === "active") done();
    });
    signal?.addEventListener("abort", done);
  });
}

/** Runs `listener` each time the app comes back to the foreground. */
export function onAppActive(listener: () => void): () => void {
  const subscription = AppState.addEventListener("change", (state) => {
    if (state === "active") listener();
  });
  return () => subscription.remove();
}

/** Doubles `base` for each failure so far, up to `max`. */
export function backoff(base: number, failures: number, max: number): number {
  return Math.min(max, base * 2 ** Math.max(0, failures));
}

/** Waits `ms`, or less when `signal` aborts. */
export function pause(ms: number, signal?: AbortSignal): Promise<void> {
  if (signal?.aborted) return Promise.resolve();
  return new Promise((resolve) => {
    const done = () => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", done);
      resolve();
    };
    const timer = setTimeout(done, ms);
    signal?.addEventListener("abort", done);
  });
}
