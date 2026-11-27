import { use, useSyncExternalStore } from "react";

import { AnimationsPaused } from "@/ui/AnimationsPaused";

// Matches the desktop's 600ms rest/talk cycle (300ms per frame).
const FRAME_MS = 300;

// One clock for every mascot at work, so they flip in one render rather than
// each on its own timer, and none runs while none is shown.
let talking = false;
let timer: ReturnType<typeof setInterval> | null = null;
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
  listeners.add(listener);
  timer ??= setInterval(() => {
    talking = !talking;
    listeners.forEach((notify) => notify());
  }, FRAME_MS);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && timer !== null) {
      clearInterval(timer);
      timer = null;
    }
  };
}
const still = () => () => {};
const current = () => talking;
const resting = () => false;

/** Flips between the rest and talk frames while `active` and not paused. */
export function useFrameToggle(active: boolean) {
  const running = active && !use(AnimationsPaused);
  return useSyncExternalStore(running ? subscribe : still, running ? current : resting);
}
