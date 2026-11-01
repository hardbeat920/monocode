import { memo } from "react";
import { StyleSheet, View } from "react-native";

import { withAlpha, themedStyles } from "@/ui/theme";

import {
  BULB,
  PIXEL_GRID,
  pixelLayers,
  pixelOffset,
  type PixelLayers,
  type PixelRect,
} from "./pixelMascots";
import { useFrameToggle } from "./useFrameToggle";

/** What a mascot's agent is doing, as on desktop. */
export type MascotStatus = "idle" | "working" | "needs-you";

const Rects = memo(function Rects({
  rects,
  unit,
}: {
  rects: readonly PixelRect[];
  unit: number;
}) {
  return (
    <>
      {rects.map(({ x, y, w, h, fill }, i) => (
        <View
          key={i}
          style={{
            position: "absolute",
            left: x * unit,
            top: y * unit,
            width: w * unit,
            height: h * unit,
            backgroundColor: fill,
          }}
        />
      ))}
    </>
  );
});

/** One frame's body and eyes; the layers are cached, so it renders once per mascot. */
const Figure = memo(function Figure({ layers, unit }: { layers: PixelLayers; unit: number }) {
  return (
    <>
      <Rects rects={layers.body} unit={unit} />
      <Rects rects={layers.open} unit={unit} />
    </>
  );
});

/**
 * Shaded pixel mascot used for monos, the native take on the desktop PixelMascot.
 * Drawn with Views from the same rects, so there is no SVG dependency. While
 * working it keeps both frames mounted and flips which one shows, so the shared
 * clock changes two styles rather than rebuilding hundreds of Views. Shows the
 * bulb when the mono needs you.
 */
export function PixelMascot({
  name,
  color,
  status = "idle",
  size = 22,
  centered = false,
}: {
  name: string;
  color: string;
  status?: MascotStatus;
  size?: number;
  /** Centres the figure itself in its square, as a picker's tiles want. */
  centered?: boolean;
}) {
  const talking = useFrameToggle(status === "working");
  const rest = pixelLayers(name, color, false);
  const talk = status === "working" ? pixelLayers(name, color, true) : null;
  const unit = size / PIXEL_GRID;
  const offset = centered ? pixelOffset(name) : { x: 0, y: 0 };

  return (
    <View style={{ width: size, height: size }} accessibilityElementsHidden>
      <View style={{ position: "absolute", left: offset.x * unit, top: offset.y * unit, width: size, height: size }}>
        {talk ? (
          <>
            <View style={talking ? styles.off : styles.frame}>
              <Figure layers={rest} unit={unit} />
            </View>
            <View style={talking ? styles.frame : styles.off}>
              <Figure layers={talk} unit={unit} />
            </View>
          </>
        ) : (
          <Figure layers={rest} unit={unit} />
        )}
      </View>
      {status === "needs-you" ? (
        <View style={{ position: "absolute", left: 12.5 * unit, top: -2.5 * unit }}>
          <Rects rects={BULB} unit={unit} />
        </View>
      ) : null}
    </View>
  );
}

/** Mascot plus the desktop's status circle in the top-right corner. */
export function MonoMascot({
  name,
  color,
  status = "idle",
  size = 22,
}: {
  name: string;
  color: string;
  status?: MascotStatus;
  size?: number;
}) {
  const styles = useStyles();
  return (
    <View style={{ width: size, height: size }}>
      <PixelMascot name={name} color={color} status={status} size={size} />
      <View
        style={[
          styles.dot,
          status === "working"
            ? { backgroundColor: color }
            : status === "needs-you"
              ? styles.dotNeedsYou
              : styles.dotIdle,
        ]}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  frame: { ...StyleSheet.absoluteFill },
  off: { ...StyleSheet.absoluteFill, display: "none" },
});

const useStyles = themedStyles((colors) => ({
  dot: {
    position: "absolute",
    top: -2,
    right: -2,
    width: 7,
    height: 7,
    borderRadius: 4,
    borderWidth: 1,
    borderColor: colors.drawer,
  },
  dotNeedsYou: {
    backgroundColor: colors.accent,
  },
  dotIdle: {
    backgroundColor: withAlpha(colors.content, 0.3),
  },
}));
