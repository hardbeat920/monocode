import { useEffect } from "react";
import { makeMutable, type SharedValue } from "react-native-reanimated";

import { useAnimationsRunning } from "./useAnimationsRunning";

/**
 * A steady count for an animation that steps through frames, kept in a shared
 * value that animated styles read on the UI thread. A step set in React state
 * commits the whole tree instead: layout over every mounted node, then a walk
 * of every mounted scroll view's subviews. Styles reading a beat must change
 * only `transform` or `opacity`, as other props still commit.
 */
export type Beat = {
  /** Starts the count, if it isn't going, until the returned function. */
  hold: () => () => void;
  counting: () => SharedValue<number>;
  /** Always 0, for what is held still. */
  resting: () => SharedValue<number>;
};

export function createBeat(periodMs: number, steps: number): Beat {
  let count: SharedValue<number> | null = null;
  let rest: SharedValue<number> | null = null;
  let step = 0;
  let holders = 0;
  let timer: ReturnType<typeof setInterval> | null = null;
  const counting = () => (count ??= makeMutable(0));
  return {
    counting,
    resting: () => (rest ??= makeMutable(0)),
    hold() {
      holders += 1;
      timer ??= setInterval(() => {
        // Counted here, as reading a shared value from JS waits on the UI thread.
        step = (step + 1) % steps;
        counting().set(step);
      }, periodMs);
      return () => {
        holders -= 1;
        if (holders === 0 && timer !== null) {
          clearInterval(timer);
          timer = null;
        }
      };
    },
  };
}

/** The beat's count while animations run here, else 0 with no clock kept going for it. */
export function useBeat(beat: Beat) {
  const running = useAnimationsRunning();
  useEffect(() => (running ? beat.hold() : undefined), [beat, running]);
  return running ? beat.counting() : beat.resting();
}
