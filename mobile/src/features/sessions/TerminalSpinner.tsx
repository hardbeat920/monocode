import { createContext, use, useSyncExternalStore } from "react";
import { Text, type StyleProp, type TextStyle } from "react-native";

// Same frames and speed as the desktop TerminalSpinner.
const FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"] as const;
const FRAME_MS = 80;

/** Holds the spinners under it still, say in a drawer that is shut. */
export const SpinnersPaused = createContext(false);

// One clock for every spinner on screen, so they step in one render rather
// than each on its own timer, and none runs while none is shown.
let frame = 0;
let timer: ReturnType<typeof setInterval> | null = null;
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
  listeners.add(listener);
  timer ??= setInterval(() => {
    frame = (frame + 1) % FRAMES.length;
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
const current = () => frame;

export function TerminalSpinner({ style }: { style?: StyleProp<TextStyle> }) {
  const paused = use(SpinnersPaused);
  const shown = useSyncExternalStore(paused ? still : subscribe, current);

  return (
    <Text accessibilityElementsHidden style={style}>
      {FRAMES[shown]}
    </Text>
  );
}
