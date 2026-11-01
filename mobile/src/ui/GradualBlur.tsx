import MaskedView from "@react-native-masked-view/masked-view";
import { BlurView } from "expo-blur";
import { LinearGradient } from "expo-linear-gradient";
import { memo, useMemo } from "react";
import { StyleSheet, View } from "react-native";

import { useScheme, withAlpha } from "./theme";

const TOP_COLORS = ["black", "rgba(0, 0, 0, 0.8)", "transparent"] as const;
const BOTTOM_COLORS = ["transparent", "rgba(0, 0, 0, 0.8)", "black"] as const;
const TINT = 0.5;

/**
 * The stacked masked blur layers. They depend on nothing but the edge, the
 * layer count, the blur and the tint, so a change of the strip's height (a
 * keyboard sliding, a header resizing) never rebuilds them: this is memoized
 * apart from the strip, and the masks and gradient arrays are made once.
 */
const Layers = memo(function Layers({
  fromTop,
  steps,
  intensity,
  tint,
  scheme,
}: {
  fromTop: boolean;
  steps: number;
  intensity: number;
  tint: string;
  scheme: "light" | "dark";
}) {
  const layers = useMemo(
    () =>
      Array.from({ length: steps }, (_, i) => {
        // Each layer starts at the edge and reaches `cutoff` of the height.
        const cutoff = 1 - i / steps;
        const locations: [number, number, number] = fromTop
          ? [0, cutoff * 0.5, cutoff]
          : [1 - cutoff, 1 - cutoff * 0.5, 1];
        return (
          <MaskedView
            key={i}
            style={StyleSheet.absoluteFill}
            maskElement={
              <LinearGradient
                colors={fromTop ? TOP_COLORS : BOTTOM_COLORS}
                locations={locations}
                style={StyleSheet.absoluteFill}
              />
            }
          >
            <BlurView
              intensity={intensity}
              tint={scheme}
              style={[StyleSheet.absoluteFill, { backgroundColor: tint }]}
            />
          </MaskedView>
        );
      }),
    [fromTop, steps, intensity, tint, scheme],
  );
  return <>{layers}</>;
});

/**
 * Blur that fades out away from a screen edge, so content scrolling under a
 * floating header or footer softens instead of being clipped. Stacks several
 * masked blur layers, each reaching a little less far, so blur and tint build
 * up toward the edge.
 * Stacks by render order: render it after the content and before the chrome.
 * Memoized, as its props are plain values and its layers costly to rebuild.
 */
export const GradualBlur = memo(function GradualBlur({
  edge,
  height,
  color,
  steps = 6,
  intensity = 6,
}: {
  edge: "top" | "bottom";
  height: number;
  /** Background the blur fades into, as `#rrggbb`. */
  color: string;
  steps?: number;
  intensity?: number;
}) {
  const fromTop = edge === "top";
  const tint = useMemo(() => withAlpha(color, TINT), [color]);
  const scheme = useScheme();

  return (
    <View
      pointerEvents="none"
      style={[styles.container, { height }, fromTop ? styles.top : styles.bottom]}
    >
      <Layers fromTop={fromTop} steps={steps} intensity={intensity} tint={tint} scheme={scheme} />
    </View>
  );
});

const styles = StyleSheet.create({
  container: {
    position: "absolute",
    left: 0,
    right: 0,
    overflow: "hidden",
  },
  top: {
    top: 0,
  },
  bottom: {
    bottom: 0,
  },
});
