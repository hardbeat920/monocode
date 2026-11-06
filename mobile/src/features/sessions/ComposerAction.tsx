import { useEffect, type ReactNode } from "react";
import { Pressable, StyleSheet, View } from "react-native";
import Animated, {
  Easing,
  FadeIn,
  FadeOut,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
  type SharedValue,
} from "react-native-reanimated";

import { ActionDisc } from "@/ui/GlassBacking";
import { Icon } from "@/ui/Icon";
import { GlassStretchButton } from "@/ui/GlassStretchButton";
import { themedStyles, useColors } from "@/ui/theme";

const SWAP = { duration: 160, easing: Easing.bezier(0.23, 1, 0.32, 1) };

/**
 * The composer's trailing control, in a lane its parent sizes: from the add
 * button's far side to the composer's end.
 *
 * It is one piece of glass. At rest that's the mic. `stretch`, going to 1,
 * draws the same glass along the lane into the capsule a recording sits in,
 * and back into the mic when it ends. Send and Stop are a tinted disc that
 * grows over the orb.
 */
export function ComposerAction({
  primary,
  dictating,
  stretch,
  blocked,
  size,
  onDictate,
  children,
}: {
  /** Send or Stop, taking over from the mic. */
  primary: {
    kind: "send" | "stop";
    label: string;
    hint?: string;
    disabled: boolean;
    onPress: () => void;
  } | null;
  dictating: boolean;
  /** How far the orb has drawn out, 0 to 1. */
  stretch: SharedValue<number>;
  blocked: boolean;
  size: number;
  onDictate: () => void;
  /** Anything over the button's corner. */
  children?: ReactNode;
}) {
  const colors = useColors();
  const styles = useStyles();
  const mic = !primary && !dictating;

  const fill = useSharedValue(primary ? 1 : 0);
  useEffect(() => {
    fill.set(withTiming(primary ? 1 : 0, SWAP));
  }, [primary, fill]);

  // A share of the lane, so nothing is measured as the card morphs. At rest it's
  // none: `minWidth` holds the orb round however wide the composer is.
  const orb = useAnimatedStyle(() => ({ width: `${100 * stretch.get()}%` }));
  const unavailable = blocked || !!primary?.disabled;

  return (
    <>
      <Animated.View
        accessibilityElementsHidden={!mic}
        importantForAccessibility={mic ? "auto" : "no-hide-descendants"}
        pointerEvents={mic ? "auto" : "none"}
        style={[styles.orb, { minWidth: size }, orb]}
      >
        <GlassStretchButton
          icon="mic"
          symbol="mic"
          label="Dictate"
          size={size}
          glyphShown={mic}
          disabled={blocked}
          onPress={onDictate}
        />
      </Animated.View>

      <Pressable
        accessibilityElementsHidden={!primary}
        importantForAccessibility={primary ? "auto" : "no-hide-descendants"}
        accessibilityRole="button"
        accessibilityLabel={primary?.label}
        accessibilityHint={primary?.hint}
        disabled={!primary || unavailable}
        hitSlop={4}
        onPress={primary?.onPress}
        pointerEvents={primary ? "auto" : "none"}
        style={[styles.slot, { width: size, height: size }]}
      >
        {({ pressed }) => (
          <>
            <ActionDisc fill={fill} size={size} quiet={unavailable} />
            {primary ? (
              <Animated.View
                key={primary.kind}
                entering={FadeIn.duration(SWAP.duration)}
                exiting={FadeOut.duration(SWAP.duration)}
                style={[styles.glyph, pressed && styles.pressed]}
              >
                {primary.kind === "send" ? (
                  <Icon name="send" size={18} color={unavailable ? colors.muted : colors.inverse} />
                ) : (
                  <View style={[styles.stop, unavailable && styles.stopQuiet]} />
                )}
              </Animated.View>
            ) : null}
          </>
        )}
      </Pressable>
      {children}
    </>
  );
}

const useStyles = themedStyles((colors) => ({
  orb: {
    position: "absolute",
    top: 0,
    right: 0,
    bottom: 0,
  },
  slot: {
    position: "absolute",
    top: 0,
    right: 0,
  },
  glyph: {
    ...StyleSheet.absoluteFill,
    alignItems: "center",
    justifyContent: "center",
  },
  stop: {
    width: 12,
    height: 12,
    borderRadius: 3,
    backgroundColor: colors.inverse,
  },
  stopQuiet: {
    backgroundColor: colors.muted,
  },
  pressed: {
    opacity: 0.6,
  },
}));
