import { use, useSyncExternalStore } from "react";
import { AppState } from "react-native";
import { useReducedMotion } from "react-native-reanimated";

import { AnimationsPaused } from "./AnimationsPaused";

function subscribe(notify: () => void) {
  const subscription = AppState.addEventListener("change", notify);
  return () => subscription.remove();
}
// Inactive too (a permission alert, the app switcher): the screen is covered.
const appActive = () =>
  AppState.currentState !== "background" && AppState.currentState !== "inactive";
const assumeActive = () => true;

/**
 * Whether an endless animation should run: not under an `AnimationsPaused`
 * (a shut drawer, a page swiped away), not with Reduce Motion on, and not
 * while the app is in the background or covered by a system alert. Loops should start when it turns true
 * and `cancelAnimation` when it turns false, so nothing spins unseen.
 */
export function useAnimationsRunning() {
  const paused = use(AnimationsPaused);
  const reduceMotion = useReducedMotion();
  const foreground = useSyncExternalStore(subscribe, appActive, assumeActive);
  return !paused && !reduceMotion && foreground;
}
