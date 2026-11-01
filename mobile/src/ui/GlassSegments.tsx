import * as Haptics from "expo-haptics";
import { useEffect, useState, type ReactNode } from "react";
import {
  Pressable,
  Text,
  View,
  type LayoutChangeEvent,
  type StyleProp,
  type ViewStyle,
} from "react-native";
import Animated, {
  Easing,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
} from "react-native-reanimated";

import { LiquidView } from "./LiquidView";
import { themedStyles } from "./theme";

const HEIGHT = 44;
const INSET = 4;
const TIMING = { duration: 240, easing: Easing.out(Easing.cubic) };

/**
 * The top switch's glass, thumb and sizes, as a full-width control whose
 * labels all stay in view: equal cells, a thumb that slides to the chosen one,
 * and an optional `trailing` node (a count, say) after a label.
 */
export function GlassSegments<T extends string>({
  segments,
  value,
  onChange,
  style,
}: {
  segments: { value: T; label: string; trailing?: ReactNode }[];
  value: T;
  onChange: (value: T) => void;
  style?: StyleProp<ViewStyle>;
}) {
  const styles = useStyles();
  const [width, setWidth] = useState(0);
  const index = Math.max(
    0,
    segments.findIndex((segment) => segment.value === value),
  );
  const position = useSharedValue(index);
  useEffect(() => {
    position.set(withTiming(index, TIMING));
  }, [index, position]);
  const cell = width / segments.length;
  const thumb = useAnimatedStyle(() => ({
    opacity: cell > 0 ? 1 : 0,
    width: cell,
    transform: [{ translateX: position.get() * cell }],
  }));
  return (
    <View style={style}>
      {/* Glass lets the rows scrolling under the switch show through; this
          keeps its track solid. */}
      <View pointerEvents="none" style={styles.solid} />
      <LiquidView
        interactive
        row={false}
        borderRadius={HEIGHT / 2}
        innerStyle={styles.track}
      >
        <View
          accessibilityRole="tablist"
          style={styles.row}
          onLayout={(event: LayoutChangeEvent) =>
            setWidth(event.nativeEvent.layout.width)
          }
        >
          <Animated.View pointerEvents="none" style={[styles.thumb, thumb]} />
          {segments.map((segment) => (
            <Pressable
              key={segment.value}
              accessibilityRole="tab"
              accessibilityState={{ selected: segment.value === value }}
              onPress={() => {
                if (segment.value === value) return;
                void Haptics.selectionAsync();
                onChange(segment.value);
              }}
              style={styles.segment}
            >
              <Text
                style={[
                  styles.label,
                  segment.value === value && styles.labelOn,
                ]}
              >
                {segment.label}
              </Text>
              {segment.trailing}
            </Pressable>
          ))}
        </View>
      </LiquidView>
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  solid: {
    position: "absolute",
    top: 0,
    bottom: 0,
    left: 0,
    right: 0,
    borderRadius: HEIGHT / 2,
    borderCurve: "continuous",
    backgroundColor: colors.surface,
  },
  track: {
    alignSelf: "stretch",
    height: HEIGHT,
    padding: INSET,
  },
  row: {
    flex: 1,
    alignSelf: "stretch",
    flexDirection: "row",
  },
  thumb: {
    position: "absolute",
    top: 0,
    bottom: 0,
    left: 0,
    borderRadius: (HEIGHT - INSET * 2) / 2,
    borderCurve: "continuous",
    backgroundColor: colors.thumb,
  },
  segment: {
    flex: 1,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: 6,
  },
  label: {
    color: colors.muted,
    fontSize: 15,
    fontWeight: "600",
  },
  labelOn: {
    color: colors.content,
  },
}));
