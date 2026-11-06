import { memo } from "react";
import { StyleSheet, View } from "react-native";
import Animated, { useAnimatedStyle } from "react-native-reanimated";

import { createBeat, useBeat } from "@/ui/beat";
import { withAlpha, themedStyles } from "@/ui/theme";

import {
  BULB,
  PIXEL_GRID,
  pixelLayers,
  pixelOffset,
  type PixelLayers,
  type PixelRect,
} from "./pixelMascots";

// Matches the desktop's 600ms rest/talk cycle (300ms per frame).
const FRAME_MS = 300;

// One clock for every mascot at work, and none while none is shown.
const beat = createBeat(FRAME_MS, 2);

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

/** Both frames mounted, the beat flipping which one shows (see `beat`). */
function Talking({ rest, talk, unit }: { rest: PixelLayers; talk: PixelLayers; unit: number }) {
  const talking = useBeat(beat);
  const resting = useAnimatedStyle(() => ({ opacity: talking.get() === 0 ? 1 : 0 }));
  const speaking = useAnimatedStyle(() => ({ opacity: talking.get() === 0 ? 0 : 1 }));
  return (
    <>
      <Animated.View collapsable={false} style={[styles.frame, resting]}>
        <Figure layers={rest} unit={unit} />
      </Animated.View>
      <Animated.View collapsable={false} style={[styles.frame, speaking]}>
        <Figure layers={talk} unit={unit} />
      </Animated.View>
    </>
  );
}

/**
 * Shaded pixel mascot used for monos, the native take on the desktop PixelMascot.
 * Drawn with Views from the same rects, so there is no SVG dependency. Shows the
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
  const rest = pixelLayers(name, color, false);
  const talk = status === "working" ? pixelLayers(name, color, true) : null;
  const unit = size / PIXEL_GRID;
  const offset = centered ? pixelOffset(name) : { x: 0, y: 0 };

  return (
    <View style={{ width: size, height: size }} accessibilityElementsHidden>
      <View style={{ position: "absolute", left: offset.x * unit, top: offset.y * unit, width: size, height: size }}>
        {talk ? (
          <Talking rest={rest} talk={talk} unit={unit} />
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

// A flip must only change a prop. `display: "none"` unmounts the frame's native
// views, and a frame left collapsable is flattened whenever it is visible, so
// either way every pixel View is rebuilt or moved on each flip.
const styles = StyleSheet.create({
  frame: { ...StyleSheet.absoluteFill },
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
