import MaskedView from "@react-native-masked-view/masked-view";
import { BlurView } from "expo-blur";
import { LinearGradient } from "expo-linear-gradient";
import { memo } from "react";
import { StyleSheet, View } from "react-native";

import { useScheme, withAlpha } from "./theme";

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
  const tint = withAlpha(color, 0.5);
  const scheme = useScheme();

  return (
    <View
      pointerEvents="none"
      style={[styles.container, { height }, fromTop ? styles.top : styles.bottom]}
    >
      {Array.from({ length: steps }, (_, i) => {
        // Each layer starts at the edge and reaches `cutoff` of the height.
        const cutoff = 1 - i / steps;
        return (
          <MaskedView
            key={i}
            style={StyleSheet.absoluteFill}
            maskElement={
              <LinearGradient
                colors={
                  fromTop
                    ? ["black", "rgba(0, 0, 0, 0.8)", "transparent"]
                    : ["transparent", "rgba(0, 0, 0, 0.8)", "black"]
                }
                locations={fromTop ? [0, cutoff * 0.5, cutoff] : [1 - cutoff, 1 - cutoff * 0.5, 1]}
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
      })}
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
