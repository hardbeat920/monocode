import { StyleSheet, Text, View, type StyleProp, type TextStyle } from "react-native";
import Animated, { useAnimatedStyle, useSharedValue } from "react-native-reanimated";

import { createBeat, useBeat } from "@/ui/beat";

// Same frames and speed as the desktop TerminalSpinner.
const FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"] as const;
const FRAME_MS = 80;

// One clock for every spinner on screen, and none while none is shown.
const beat = createBeat(FRAME_MS, FRAMES.length);

/** Every frame on one strip, slid past a window a line tall (see `beat`). */
export function TerminalSpinner({ style }: { style?: StyleProp<TextStyle> }) {
  const frame = useBeat(beat);
  const line = useSharedValue(0);
  const strip = useAnimatedStyle(() => ({
    transform: [{ translateY: -frame.get() * line.get() }],
  }));

  return (
    <View style={styles.window} accessibilityElementsHidden>
      {/* Sizes the window as the single frame it shows. */}
      <Text style={[style, styles.unseen]}>{FRAMES[0]}</Text>
      <Animated.View style={[styles.strip, strip]}>
        <Text
          style={style}
          onLayout={(event) => line.set(event.nativeEvent.layout.height / FRAMES.length)}
        >
          {FRAMES.join("\n")}
        </Text>
      </Animated.View>
    </View>
  );
}

const styles = StyleSheet.create({
  window: {
    overflow: "hidden",
  },
  unseen: {
    opacity: 0,
  },
  strip: {
    position: "absolute",
    top: 0,
    left: 0,
    right: 0,
  },
});
