import { isLiquidGlassAvailable } from "expo-glass-effect";
import { Pressable, Text, type LayoutChangeEvent, type StyleProp, type ViewStyle } from "react-native";
import Animated, {
  interpolate,
  useAnimatedStyle,
  useSharedValue,
  type AnimatedStyle,
  type SharedValue,
} from "react-native-reanimated";

import type { Harness } from "@/features/workspace/types";
import { GlassBacking } from "@/ui/GlassBacking";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

import { ProviderLogo } from "./ProviderLogo";

/** How far the card has opened when the glass is fully out, ahead of the words on it. */
const DRAWN_BY = 0.75;
const GAP = 8;

type Frame = { x: number; width: number };

/**
 * The model, along the composer's bottom row past the add button.
 *
 * Where there's Liquid Glass it's glass, the add button's material. Glass can't
 * be faded (see LiquidView's GlassPresence), so it travels instead: it draws out
 * from behind the add button as the card opens, and back in as it folds or a
 * recording takes the row. What's written on it does fade, and lays out on its
 * own row, so the glass follows the frame that row gives it, truncation included.
 * Fully away it's taken off screen rather than left under the add button. The
 * touch is the row's own, as it is without glass; the glass under it takes none.
 */
export function ComposerPills({
  progress,
  dictation,
  written,
  size,
  edge,
  model,
  effort,
  harness,
  onModelPress,
}: {
  /** The card's morph, 0 as a pill to 1 open. */
  progress: SharedValue<number>;
  /** A recording taking over the row, 0 to 1. */
  dictation: SharedValue<number>;
  /** Fades what's written here along with the rest of the card. */
  written: StyleProp<AnimatedStyle<StyleProp<ViewStyle>>>;
  size: number;
  /** Gap from the composer's edge to its round buttons. */
  edge: number;
  model: string;
  effort?: string;
  harness?: Harness;
  onModelPress?: () => void;
}) {
  const styles = useStyles();
  const colors = useColors();
  const lane = edge + size + GAP;
  const glass = isLiquidGlassAvailable();

  const modelFrame = useSharedValue<Frame>({ x: 0, width: 0 });
  const modelGlass = useAnimatedStyle(() => {
    const t = interpolate(progress.get(), [0, DRAWN_BY], [0, 1], "clamp") * (1 - dictation.get());
    const { x, width } = modelFrame.get();
    return {
      display: t === 0 ? "none" : "flex",
      left: edge + (lane + x - edge) * t,
      width: size + (width - size) * t,
    };
  });

  const trackModel = (event: LayoutChangeEvent) => {
    const { x, width } = event.nativeEvent.layout;
    modelFrame.set({ x, width });
  };

  return (
    <>
      {glass ? (
        <Animated.View style={[styles.glass, { bottom: edge, height: size }, modelGlass]}>
          <GlassBacking />
        </Animated.View>
      ) : null}
      <Animated.View style={[styles.row, { left: lane, right: lane, bottom: edge }, written]}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`Model: ${model || "Desktop default"}${effort ? `, ${effort} effort` : ""}`}
          accessibilityHint="Choose the model, account, effort and permissions"
          onLayout={trackModel}
          onPress={onModelPress}
          style={({ pressed }) => [
            styles.modelPill,
            !glass && styles.filled,
            { height: size, borderRadius: size / 2 },
            pressed && styles.pressed,
          ]}
        >
          {harness ? <ProviderLogo harness={harness} size={16} color={colors.content} /> : null}
          <Text style={styles.model} numberOfLines={1}>
            {model || "Desktop default"}
            {effort ? <Text style={styles.effort}> {effort}</Text> : null}
          </Text>
          <Icon name="chevronDown" size={14} color={colors.muted} />
        </Pressable>
      </Animated.View>
    </>
  );
}

const useStyles = themedStyles((colors) => ({
  row: {
    position: "absolute",
    flexDirection: "row",
  },
  glass: {
    position: "absolute",
  },
  // Shaped like the round buttons beside it: their height and full rounding.
  modelPill: {
    flexShrink: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingHorizontal: 12,
  },
  filled: {
    backgroundColor: colors.fill,
  },
  model: {
    flexShrink: 1,
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
  },
  effort: {
    color: colors.muted,
    fontWeight: "400",
  },
  pressed: {
    opacity: 0.6,
  },
}));
