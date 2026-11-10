import { createContext, use, useEffect, useRef, useState, type ReactNode } from "react";
import { StyleSheet, View } from "react-native";
import Animated, {
  Easing,
  useAnimatedReaction,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
  type SharedValue,
} from "react-native-reanimated";
import { scheduleOnRN } from "react-native-worklets";

// Eases in as well as out: a curve that does most of its travel at once
// spends it while a first opening is still laying its content out.
const TIMING = { duration: 280, easing: Easing.bezier(0.4, 0, 0.2, 1) };

/**
 * What a list around collapsibles lends them: told when one starts and stops
 * moving, and, under an anchored message, the room left below it, which they
 * give and take frame by frame as they open and fold so the list's end moves
 * with them rather than a few frames behind.
 */
export type CollapseMotion = {
  slack: SharedValue<number> | null;
  begin: () => void;
  end: () => void;
};

export const CollapseMotionContext = createContext<CollapseMotion | null>(null);

/**
 * Content that opens down from its top and folds back up, fading as it goes.
 * Open, it follows its content's height, so what it holds can grow. Once
 * opened it keeps its content, hidden, so opening again starts at once.
 */
export function Collapsible({ open, children }: { open: boolean; children: ReactNode }) {
  const motion = use(CollapseMotionContext);
  const slack = motion?.slack ?? null;
  const [isMounted, setMounted] = useState(open);
  const mounted = useRef(open);
  // Opening waits for the content's first layout, so it has a height to open
  // to: mounting a busy transcript's rows can take longer than the animation.
  const opening = useRef(false);
  const first = useRef(true);
  const progress = useSharedValue(open ? 1 : 0);
  const height = useSharedValue(0);

  const animate = (to: 0 | 1) => {
    motion?.begin();
    const end = motion?.end;
    progress.set(
      withTiming(to, TIMING, () => {
        if (end) scheduleOnRN(end);
      }),
    );
  };

  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    if (!open) {
      opening.current = false;
      animate(0);
    } else if (mounted.current) {
      animate(1);
    } else {
      mounted.current = true;
      opening.current = true;
      setMounted(true);
    }
    // Runs on a toggle only; animate reads the latest motion as it is.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // What the animation adds or takes each frame comes out of the room below,
  // on the UI thread with the height itself. A new measurement is the list's
  // to account for, from its rows' layout.
  useAnimatedReaction(
    () => [height.get(), progress.get()] as const,
    (current, previous) => {
      if (slack === null || previous === null || current[0] !== previous[0]) return;
      slack.set(slack.get() - current[0] * (current[1] - previous[1]));
    },
    [slack],
  );

  const style = useAnimatedStyle(() => ({
    height: height.get() * progress.get(),
    opacity: progress.get(),
  }));
  if (!isMounted) return null;
  return (
    <Animated.View
      style={[styles.frame, style]}
      pointerEvents={open ? "auto" : "none"}
      accessibilityElementsHidden={!open}
      importantForAccessibility={open ? "auto" : "no-hide-descendants"}
    >
      {/* Out of the flow, so it measures its own height however far open the
          frame is; laid out in it, it measured the frame's 0 and opened to
          nothing before snapping open. */}
      <View
        style={styles.content}
        onLayout={(event) => {
          height.set(event.nativeEvent.layout.height);
          if (!opening.current) return;
          opening.current = false;
          animate(1);
        }}
      >
        {children}
      </View>
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  frame: {
    overflow: "hidden",
  },
  content: {
    position: "absolute",
    top: 0,
    left: 0,
    right: 0,
  },
});
