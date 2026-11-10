import { useNavigation } from "expo-router";
import { useEffect, useState } from "react";

/** Long enough for any push; for a screen whose transition never reports. */
const PUSH_FALLBACK_MS = 600;
/** How long a push takes to start once its screen is mounted, at worst. */
const START_FALLBACK_MS = 300;

/**
 * True once this screen's push has finished, for work that would otherwise
 * land on the JS thread as it slides in: reads from the desktop, say, whose
 * replies re-render the screen. Not `pushed`, it is true at once.
 */
export function usePushedIn(pushed: boolean) {
  return useTransition(pushed, "transitionEnd", PUSH_FALLBACK_MS);
}

/**
 * True once this screen's push has started, for work that would otherwise
 * hold it back: the slide waits for the JS thread to go quiet. Not `pushed`,
 * it is true at once.
 */
export function usePushStarted(pushed: boolean) {
  return useTransition(pushed, "transitionStart", START_FALLBACK_MS);
}

function useTransition(
  pushed: boolean,
  event: "transitionStart" | "transitionEnd",
  fallbackMs: number,
) {
  const navigation = useNavigation();
  const [done, setDone] = useState(!pushed);
  useEffect(() => {
    if (done) return;
    const finish = () => setDone(true);
    const timer = setTimeout(finish, fallbackMs);
    const remove = navigation.addListener(event as never, finish);
    return () => {
      clearTimeout(timer);
      remove();
    };
  }, [done, navigation, event, fallbackMs]);
  return done;
}
