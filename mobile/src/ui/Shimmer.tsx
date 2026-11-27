import MaskedView from "@react-native-masked-view/masked-view";
import { LinearGradient } from "expo-linear-gradient";
import { useEffect, useState } from "react";
import {
  StyleSheet,
  Text,
  View,
  type StyleProp,
  type TextStyle,
} from "react-native";
import Animated, {
  Easing,
  useAnimatedStyle,
  useSharedValue,
  withRepeat,
  withTiming,
} from "react-native-reanimated";

import { withAlpha, useColors } from "@/ui/theme";

/** Each character widens the light, as the desktop's `--spread` does. */
const SPREAD_PER_CHAR = 2;

/**
 * Live text with the desktop's shimmer (`.shimmer-text`): a band of light
 * sweeping left to right through the text, from a quarter of its width before
 * it to a quarter after, over its own colour. One line, cut short as needed.
 */
export function Shimmer({
  children,
  style,
  duration = 1.6,
}: {
  children: string;
  style: StyleProp<TextStyle>;
  /** Seconds per sweep. */
  duration?: number;
}) {
  const colors = useColors();
  // The slot is the label's; the text inside sizes to its words.
  const { flex, flexShrink, flexGrow, alignSelf, ...text } =
    StyleSheet.flatten(style) ?? {};
  const [width, setWidth] = useState(0);
  const spread = children.length * SPREAD_PER_CHAR;

  const progress = useSharedValue(0);
  useEffect(() => {
    progress.set(
      withRepeat(
        withTiming(1, { duration: duration * 1000, easing: Easing.linear }),
        -1,
        false,
      ),
    );
  }, [progress, duration]);
  const band = useAnimatedStyle(() => ({
    transform: [
      { translateX: width * (-0.25 + 1.5 * progress.get()) - spread },
    ],
  }));

  const line = (
    <Text style={text} numberOfLines={1}>
      {children}
    </Text>
  );
  return (
    <View style={[styles.slot, { flex, flexShrink, flexGrow, alignSelf }]}>
      <MaskedView
        style={styles.mask}
        maskElement={line}
        accessible
        accessibilityLabel={children}
      >
        {/* Lays the mask out and stays unseen; the fill and light show through it. */}
        <Text
          style={[text, styles.hidden]}
          numberOfLines={1}
          onTextLayout={(event) =>
            setWidth(Math.ceil(event.nativeEvent.lines[0]?.width ?? 0))
          }
        >
          {children}
        </Text>
        <View
          style={[StyleSheet.absoluteFill, { backgroundColor: text.color }]}
        />
        <Animated.View
          style={[styles.band, { width: spread * 2 }, band]}
          pointerEvents="none"
        >
          <LinearGradient
            // Its edges are its own colour gone clear, as "transparent"
            // fades through black.
            colors={[withAlpha(colors.content, 0), colors.content, withAlpha(colors.content, 0)]}
            start={{ x: 0, y: 0.5 }}
            end={{ x: 1, y: 0.5 }}
            style={StyleSheet.absoluteFill}
          />
        </Animated.View>
      </MaskedView>
    </View>
  );
}

const styles = StyleSheet.create({
  slot: {
    minWidth: 0,
    flexDirection: "row",
  },
  mask: {
    flexShrink: 1,
  },
  hidden: {
    opacity: 0,
  },
  band: {
    position: "absolute",
    top: 0,
    bottom: 0,
    left: 0,
  },
});
