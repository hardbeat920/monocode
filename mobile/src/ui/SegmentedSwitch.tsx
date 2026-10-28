import * as Haptics from "expo-haptics";
import { useEffect, useState, type ReactNode } from "react";
import { Platform, Pressable, View, type LayoutChangeEvent } from "react-native";
import Animated, {
  Easing,
  interpolate,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
  type SharedValue,
} from "react-native-reanimated";

import { LiquidView } from "./LiquidView";
import { themedStyles } from "./theme";

const HEIGHT = 44;
const INSET = 4;
const PAD_COLLAPSED = 12;
const PAD_EXPANDED = 16;
const GAP = 8;
/** Unselected icons dim back so the selected segment reads first. */
const ICON_DIMMED = 0.5;
/** Long project names cut off with an ellipsis rather than stretching the switch. */
const LABEL_MAX = 120;
const TIMING = { duration: 240, easing: Easing.out(Easing.cubic) };

/**
 * Widths measured by any switch, by what was measured, so one that mounts
 * again (each time its screen opens) is its full size from the first frame
 * rather than growing into it as it measures, mid fade in.
 */
const measured = new Map<string, number>();
const iconKey = (value: string) => `icon:${value}`;
const labelKey = (label: string) => `label:${label}`;

/** How selected segment `index` is at `progress`: 1 on it, 0 a segment or more away. */
function activeness(progress: number, index: number) {
  "worklet";
  return Math.max(0, 1 - Math.abs(progress - index));
}

/**
 * Glass segmented control, like the Chat / Work switch in the ChatGPT app: a
 * lighter thumb slides behind the selected segment. Unselected segments show
 * only their icon; the selected one reveals its label, like the tabs in desktop
 * mono chats. Everything follows one continuous `progress` (the selected
 * index), so a parent that pages its content by swipe can pass its own and the
 * thumb and labels move with the finger. Without one, a tap animates there.
 */
export function SegmentedSwitch<T extends string>({
  segments,
  value,
  onChange,
  progress: external,
}: {
  segments: { value: T; label: string; icon: ReactNode }[];
  value: T;
  onChange: (value: T) => void;
  progress?: SharedValue<number>;
}) {
  const styles = useStyles();
  const selectedIndex = Math.max(
    0,
    segments.findIndex((segment) => segment.value === value),
  );
  const own = useSharedValue(selectedIndex);
  useEffect(() => {
    if (!external) own.set(withTiming(selectedIndex, TIMING));
  }, [external, own, selectedIndex]);
  const progress = external ?? own;

  // Natural widths, so each segment's width is a function of progress alone.
  const [iconWidths, setIconWidths] = useState<number[]>(() =>
    known(segments.map((segment) => iconKey(segment.value))),
  );
  const [labelWidths, setLabelWidths] = useState<number[]>(() =>
    known(segments.map((segment) => labelKey(segment.label))),
  );
  const measure = (set: typeof setIconWidths, index: number, key: string) => (event: LayoutChangeEvent) => {
    const width = Math.ceil(event.nativeEvent.layout.width);
    measured.set(key, width);
    set((prev) => {
      if (prev[index] === width) return prev;
      const next = [...prev];
      next[index] = width;
      return next;
    });
  };

  const count = segments.length;
  const thumbStyle = useAnimatedStyle(() => {
    const p = Math.min(count - 1, Math.max(0, progress.get()));
    const widths: number[] = [];
    for (let i = 0; i < count; i++) {
      const a = activeness(p, i);
      const pad = PAD_COLLAPSED + (PAD_EXPANDED - PAD_COLLAPSED) * a;
      widths.push(pad * 2 + (iconWidths[i] ?? 0) + ((labelWidths[i] ?? 0) + GAP) * a);
    }
    const from = Math.min(count - 1, Math.floor(p));
    const to = Math.min(count - 1, from + 1);
    const t = p - from;
    let left = 0;
    for (let i = 0; i < from; i++) left += widths[i];
    return {
      opacity: iconWidths.length === count ? 1 : 0,
      transform: [{ translateX: left + widths[from] * t }],
      width: widths[from] * (1 - t) + widths[to] * t,
    };
  });

  return (
    <LiquidView
      // Android's native title container fills the available header width.
      // Keep the glass at the segments' natural width inside that container.
      style={Platform.OS === "android" ? styles.compact : undefined}
      innerStyle={styles.track}
    >
      <View style={styles.row}>
        <Animated.View pointerEvents="none" style={[styles.thumb, thumbStyle]} />
        {segments.map((segment, index) => (
          <Segment
            key={segment.value}
            index={index}
            progress={progress}
            label={segment.label}
            icon={segment.icon}
            labelWidth={labelWidths[index] ?? 0}
            selected={index === selectedIndex}
            onIconLayout={measure(setIconWidths, index, iconKey(segment.value))}
            onLabelLayout={measure(setLabelWidths, index, labelKey(segment.label))}
            onPress={() => {
              if (index === selectedIndex) return;
              void Haptics.selectionAsync();
              onChange(segment.value);
            }}
          />
        ))}
      </View>
    </LiquidView>
  );
}

/** The widths already measured for `keys`; empty unless all of them are. */
function known(keys: string[]) {
  const widths = keys.map((key) => measured.get(key));
  return widths.every((width) => width !== undefined) ? (widths as number[]) : [];
}

function Segment({
  index,
  progress,
  label,
  icon,
  labelWidth,
  selected,
  onIconLayout,
  onLabelLayout,
  onPress,
}: {
  index: number;
  progress: SharedValue<number>;
  label: string;
  icon: ReactNode;
  labelWidth: number;
  selected: boolean;
  onIconLayout: (event: LayoutChangeEvent) => void;
  onLabelLayout: (event: LayoutChangeEvent) => void;
  onPress: () => void;
}) {
  const styles = useStyles();
  const segmentStyle = useAnimatedStyle(() => ({
    paddingHorizontal:
      PAD_COLLAPSED + (PAD_EXPANDED - PAD_COLLAPSED) * activeness(progress.get(), index),
  }));
  const iconStyle = useAnimatedStyle(() => ({
    opacity: interpolate(activeness(progress.get(), index), [0, 1], [ICON_DIMMED, 1]),
  }));
  // The label sits at its natural width inside a window that opens with the
  // segment, so it's uncovered rather than squeezed. It fades in over the back
  // half of the opening, once there's room for it, and out over the front half.
  const labelStyle = useAnimatedStyle(() => {
    const a = activeness(progress.get(), index);
    return {
      width: (labelWidth + GAP) * a,
      opacity: interpolate(a, [0.4, 1], [0, 1], "clamp"),
    };
  });

  return (
    <Pressable
      accessibilityRole="tab"
      accessibilityLabel={label}
      accessibilityState={{ selected }}
      onPress={onPress}
    >
      <Animated.View style={[styles.segment, segmentStyle]}>
        <Animated.View onLayout={onIconLayout} style={iconStyle}>
          {icon}
        </Animated.View>
        <Animated.View style={[styles.labelWindow, labelStyle]}>
          <View style={styles.labelMeasure}>
            <Animated.Text
              onLayout={onLabelLayout}
              numberOfLines={1}
              ellipsizeMode="tail"
              style={styles.label}
            >
              {label}
            </Animated.Text>
          </View>
        </Animated.View>
      </Animated.View>
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  compact: {
    alignSelf: "center",
  },
  track: {
    height: HEIGHT,
    padding: INSET,
  },
  row: {
    flexDirection: "row",
    height: HEIGHT - INSET * 2,
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
    height: HEIGHT - INSET * 2,
    flexDirection: "row",
    alignItems: "center",
  },
  labelWindow: {
    height: "100%",
    overflow: "hidden",
  },
  // Wide enough for any label, so the text lays out at its natural width
  // whatever the window around it is doing.
  labelMeasure: {
    position: "absolute",
    top: 0,
    bottom: 0,
    left: GAP,
    width: LABEL_MAX + 1,
    flexDirection: "row",
    alignItems: "center",
  },
  label: {
    maxWidth: LABEL_MAX,
    color: colors.content,
    fontSize: 15,
    fontWeight: "600",
  },
}));
