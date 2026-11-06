import { Host, HStack, Spacer } from "@expo/ui/swift-ui";
import { frame, glassEffect } from "@expo/ui/swift-ui/modifiers";
import { isLiquidGlassAvailable } from "expo-glass-effect";
import { StyleSheet } from "react-native";
import Animated, { useAnimatedStyle, type SharedValue } from "react-native-reanimated";

import { themedStyles, useColors } from "./theme";

/** Wider and taller than any screen: the glass fills what the Host gives it. */
const FILL = 10000;
/** A solid disc starts this small, so it grows out of what's under it. */
const SOLID_FROM = 0.7;

/**
 * Glass behind something that isn't itself SwiftUI, filling its parent. It takes
 * no touches: a Pressable over it does, so it presses and reads to a screen
 * reader as any other control here. It's the material of GlassMenuButton, which
 * the app's own GlassView doesn't match. Give `radius` for rounded corners, or
 * `round` for a circle; otherwise it's a capsule. `tint` colours the glass. Only
 * where `isLiquidGlassAvailable()`.
 *
 * The Host ignores the keyboard's safe area, for the reason GlassMenuButton's do.
 */
export function GlassBacking({
  radius,
  round = false,
  tint,
}: {
  radius?: number;
  round?: boolean;
  tint?: string;
}) {
  return (
    <Host ignoreSafeArea="keyboard" pointerEvents="none" style={StyleSheet.absoluteFill}>
      <HStack
        modifiers={[
          frame({ maxWidth: FILL, maxHeight: FILL }),
          glassEffect({
            glass: { variant: "regular", tint },
            shape: round ? "circle" : radius === undefined ? "capsule" : "roundedRectangle",
            cornerRadius: radius,
          }),
        ]}
      >
        <Spacer />
      </HStack>
    </Host>
  );
}

/**
 * The disc behind a primary action, Send and the like, growing in over what's
 * under it as `fill` goes to 1. Tinted glass where there's Liquid Glass, which
 * can't be faded, so it grows from nothing and leaves the screen at 0. Solid
 * elsewhere. `quiet` drops the tint, for an action that's unavailable: glass
 * ignores the opacity that would dim it.
 */
export function ActionDisc({
  fill,
  size,
  quiet = false,
}: {
  fill: SharedValue<number>;
  size: number;
  quiet?: boolean;
}) {
  const colors = useColors();
  const styles = useStyles();
  const glass = isLiquidGlassAvailable();

  const grown = useAnimatedStyle(() => {
    const t = fill.get();
    const inset = (size * (1 - t)) / 2;
    return { display: t === 0 ? "none" : "flex", top: inset, left: inset, right: inset, bottom: inset };
  });
  const faded = useAnimatedStyle(() => ({
    opacity: fill.get(),
    transform: [{ scale: SOLID_FROM + (1 - SOLID_FROM) * fill.get() }],
  }));

  if (glass) {
    return (
      <Animated.View style={[styles.grown, grown]}>
        <GlassBacking round tint={quiet ? undefined : colors.inverseSurface} />
      </Animated.View>
    );
  }
  return <Animated.View style={[styles.solid, { borderRadius: size / 2 }, faded]} />;
}

const useStyles = themedStyles((colors) => ({
  grown: {
    position: "absolute",
  },
  solid: {
    ...StyleSheet.absoluteFill,
    backgroundColor: colors.inverseSurface,
  },
}));
