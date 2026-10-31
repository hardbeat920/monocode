import { use, useSyncExternalStore } from "react";
import { Text, type StyleProp, type TextStyle } from "react-native";

import { AnimationsPaused } from "@/ui/AnimationsPaused";

// Same frames and speed as the desktop TerminalSpinner.
const FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"] as const;
const FRAME_MS = 80;

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
  const paused = use(AnimationsPaused);
  const shown = useSyncExternalStore(paused ? still : subscribe, current);

  return (
    <Text accessibilityElementsHidden style={style}>
      {FRAMES[shown]}
    </Text>
  );
}
