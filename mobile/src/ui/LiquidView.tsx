import { BlurView } from "expo-blur";
import { GlassView, isLiquidGlassAvailable } from "expo-glass-effect";
import * as Haptics from "expo-haptics";
import { LinearGradient } from "expo-linear-gradient";
import {
  createContext,
  use,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import {
  Pressable,
  Platform,
  StyleSheet,
  type View,
  type StyleProp,
  type ViewStyle,
} from "react-native";
import Animated, {
  Easing,
  useAnimatedStyle,
  useSharedValue,
  withSpring,
  withTiming,
} from "react-native-reanimated";

import { themedStyles, useScheme, withAlpha } from "./theme";

// Bouncy iOS-style press spring.
const SPRING = { damping: 15, stiffness: 100, mass: 0.8 };

// Hairline border that catches light on the top-left and bottom-right.
const BORDER_GRADIENT = [
  "rgba(255, 255, 255, 0.1)",
  "rgba(255, 255, 255, 0)",
  "rgba(255, 255, 255, 0)",
  "rgba(255, 255, 255, 0.09)",
] as const;

/**
 * Set where glass should come and go with the screen, say the header's: true
 * forms it, false clears it, both animated natively. Opacity can't do this, as
 * the glass keeps showing through a faded view.
 */
export const GlassPresence = createContext<{
  shown: boolean;
  /** Seconds. */
  duration: number;
} | null>(null);

/** Android surfaces explicitly sample this content, excluding their own UI. */
export const GlassBlurTarget = createContext<
  RefObject<View | null> | undefined
>(undefined);

/** A spring with some bounce, for glass popping up into place. */
export const POP_SPRING = { damping: 14, stiffness: 190, mass: 0.9 };
/** How far below its place popping glass starts, and how small. */
export const POP_FROM = 40;
export const POP_SCALE = 0.85;

/**
 * Pops a view up from below into place while `visible`, overshooting a
 * little, and drops it back as it goes. Never quite clear, or its glass comes
 * up without its effect (see GLASS_MIN_OPACITY in SceneFade).
 */
export function usePopStyle(
  visible: boolean,
  duration: number,
  { shrinkOnExit = true }: { shrinkOnExit?: boolean } = {},
) {
  const pop = useSharedValue(0);
  useEffect(() => {
    pop.set(
      visible
        ? withSpring(1, POP_SPRING)
        : withTiming(0, { duration, easing: Easing.in(Easing.cubic) }),
    );
  }, [visible, duration, pop]);
  return useAnimatedStyle(() => {
    const t = pop.get();
    return {
      opacity: Math.max(0.03, Math.min(1, t / 0.4)),
      transform: [
        { translateY: (1 - t) * POP_FROM },
        { scale: visible || shrinkOnExit ? POP_SCALE + (1 - POP_SCALE) * t : 1 },
      ],
    };
  });
}

/**
 * For glass that comes and goes with `shown`: whether to render it, whether
 * it's in (for the content's own fade), and the GlassPresence to wrap it in.
 * It renders cleared and forms a couple of frames later, as glass that's
 * made already formed just appears; going, it clears and is let go once
 * `duration` (ms) is up.
 */
export function useGlassEntrance(shown: boolean, duration: number) {
  const [mounted, setMounted] = useState(shown);
  const [formed, setFormed] = useState(false);
  if (shown && !mounted) setMounted(true);
  if (!mounted && formed) setFormed(false);
  useEffect(() => {
    if (!shown) {
      const timer = setTimeout(() => setMounted(false), duration);
      return () => clearTimeout(timer);
    }
    if (formed) return;
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => setFormed(true));
    });
    return () => cancelAnimationFrame(frame);
  }, [shown, formed, duration]);
  const visible = shown && formed;
  const presence = useMemo(
    () => ({ shown: visible, duration: duration / 1000 }),
    [visible, duration],
  );
  return { mounted, visible, presence };
}

/**
 * Liquid Glass surface for buttons, inputs and pills. Uses the native glass
 * effect on iOS 26+, and a blurred surface with a gradient hairline border
 * everywhere else. Interactive when `onPress` is set: springs on press and
 * gives light haptic feedback.
 */
export function LiquidView({
  children,
  onPress,
  onPressIn,
  accessibilityLabel,
  pressedScale = 1.05,
  haptics = true,
  borderRadius = 999,
  row = true,
  tintColor,
  disabled = false,
  style,
  innerStyle,
  fill = false,
}: {
  children: ReactNode;
  onPress?: () => void;
  /** Fires on touch down, e.g. to keep the keyboard open by refocusing an input. */
  onPressIn?: () => void;
  accessibilityLabel?: string;
  /** Scale while pressed. */
  pressedScale?: number;
  haptics?: boolean;
  borderRadius?: number;
  /** Lays children out in a row; set false for inputs and stacked content. */
  row?: boolean;
  /** Glass tint, or the solid fill of the fallback surface. */
  tintColor?: string;
  disabled?: boolean;
  /** Outer, animated container. */
  style?: StyleProp<ViewStyle>;
  /** The glass surface itself: padding, size, alignment. */
  innerStyle?: StyleProp<ViewStyle>;
  /** Stretch the glass to the outer container's size, e.g. one whose height animates. */
  fill?: boolean;
}) {
  const styles = useStyles();
  const scale = useSharedValue(1);
  const animatedStyle = useAnimatedStyle(() => ({
    transform: [{ scale: scale.get() }],
  }));
  const interactive = !!onPress && !disabled;
  const presence = use(GlassPresence);
  const scheme = useScheme();
  const blurTarget = use(GlassBlurTarget);
  const androidBlur = Platform.OS === "android" && !!blurTarget;

  const surface: StyleProp<ViewStyle> = [
    styles.surface,
    { borderRadius, flexDirection: row ? "row" : "column" },
    fill && styles.fill,
    innerStyle,
  ];

  const content = isLiquidGlassAvailable() ? (
    <GlassView
      glassEffectStyle={
        presence
          ? {
              style: presence.shown ? "regular" : "none",
              animate: true,
              animationDuration: presence.duration,
            }
          : "regular"
      }
      isInteractive={interactive}
      tintColor={tintColor}
      style={surface}
    >
      {children}
    </GlassView>
  ) : (
    <LinearGradient
      colors={BORDER_GRADIENT}
      style={[styles.border, { borderRadius }, fill && styles.fill]}
    >
      <BlurView
        blurTarget={androidBlur ? blurTarget : undefined}
        blurMethod={androidBlur ? "dimezisBlurViewSdk31Plus" : undefined}
        intensity={30}
        tint={scheme}
        style={[
          surface,
          styles.fallback,
          androidBlur && styles.androidBlur,
          tintColor ? { backgroundColor: tintColor } : null,
        ]}
      >
        {children}
      </BlurView>
    </LinearGradient>
  );

  if (!onPress) {
    return <Animated.View style={style}>{content}</Animated.View>;
  }

  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel}
      accessibilityState={{ disabled }}
      disabled={disabled}
      hitSlop={4}
      onPressIn={() => {
        onPressIn?.();
        scale.set(withSpring(pressedScale, SPRING));
        if (haptics)
          void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
      }}
      onPressOut={() => {
        scale.set(withSpring(1, SPRING));
      }}
      onPress={onPress}
    >
      <Animated.View style={[animatedStyle, style]}>{content}</Animated.View>
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  surface: {
    alignItems: "center",
    justifyContent: "center",
    overflow: "hidden",
    borderCurve: "continuous",
  },
  fill: {
    flex: 1,
  },
  border: {
    padding: StyleSheet.hairlineWidth * 2,
  },
  fallback: {
    backgroundColor: withAlpha(colors.surface, 0.92),
  },
  androidBlur: {
    backgroundColor: withAlpha(colors.surface, 0.45),
  },
}));
