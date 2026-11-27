import { useEffect } from "react";
import { Image, PixelRatio, StyleSheet } from "react-native";
import { scheduleOnRN } from "react-native-worklets";
import Animated, {
  Easing,
  withDelay,
  useAnimatedStyle,
  useReducedMotion,
  useSharedValue,
  withRepeat,
  withTiming,
} from "react-native-reanimated";

import { encodePng } from "@/features/background/dither";

import { traceSwitch } from "./ScenePager";
import { useColors } from "./theme";

// A screen's content loading, in the desktop charts' dither: a ring of 3pt
// cells inked through the 4x4 Bayer pattern (crates/monocode-gpui/src/visuals/
// dither.rs), its bright head sweeping round from twelve o'clock as the
// charts' donut does, its tail thinning into scattered cells behind it, in the
// desktop spinner's slate, peach and pink.
//
// It shows while the drawer shuts, so nothing in it may load the JS thread.
// Its frames are drawn once, side by side in one strip, and the strip steps a
// whole frame at a time: a transform on the UI thread, cells always crisp,
// where turning one image would smear them.

/** The charts' cell, in points. */
const CELL = 3;
/** The ring's square, in cells, and its band. */
const CELLS = 24;
const INNER = 6.2;
const OUTER = 11.2;
const SIZE = CELLS * CELL;
/** Frames in a turn, and how long a turn takes. */
const FRAMES = 24;
const TURN_MS = 1200;
/** The cover's arrival: its ground fades in as the drawer starts to shut, the
 *  ring a beat after. Its leaving, in turn: the ring fades and shrinks as it
 *  spins, then the ground fades off the next screen. */
const GROUND_IN = { duration: 150, easing: Easing.out(Easing.quad) };
const RING_IN = { delay: 70, duration: 180, easing: Easing.out(Easing.quad) };
const RING_OUT = { duration: 150, easing: Easing.in(Easing.quad) };
const GROUND_OUT = { delay: 80, duration: 200, easing: Easing.inOut(Easing.quad) };
/** How small the ring gets as it goes. */
const RING_GONE_SCALE = 0.7;
const BAYER4 = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
/** The desktop's working spinner's inks, top row to bottom
 *  (crates/monocode-gpui/src/session_mark.rs): slate, peach and pink. The
 *  ring takes them top to bottom too, so its head runs through all three. */
const INKS = ["#6f84a3", "#e8b583", "#e47fa6"];

/** Strips drawn so far, by colour and pixel density. */
const strips = new Map<string, { uri: string; scale: number }>();

function rgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Every frame of the ring, left to right, on a clear ground. */
function strip() {
  const scale = Math.round(PixelRatio.get());
  const key = String(scale);
  const cached = strips.get(key);
  if (cached) return cached;
  const cell = CELL * scale;
  const side = SIZE * scale;
  const width = side * FRAMES;
  const pixels = new Uint8Array(width * side * 4);
  const inks = INKS.map(rgb);
  // Each cell's ink by its height in the ring, the neighbouring two blended
  // through a dither of their own, offset from the tone's.
  const inkAt = (row: number, col: number) => {
    const t = ((row + 0.5) / CELLS) * (inks.length - 1);
    const i = Math.min(Math.floor(t), inks.length - 2);
    const mix = (BAYER4[((row + 2) & 3) * 4 + ((col + 1) & 3)] + 0.5) / 16;
    return inks[t - i > mix ? i + 1 : i];
  };
  const middle = (INNER + OUTER) / 2;
  const half = (OUTER - INNER) / 2;
  for (let frame = 0; frame < FRAMES; frame++) {
    const head = (2 * Math.PI * frame) / FRAMES;
    for (let row = 0; row < CELLS; row++) {
      for (let col = 0; col < CELLS; col++) {
        const dx = col + 0.5 - CELLS / 2;
        const dy = row + 0.5 - CELLS / 2;
        const r = Math.hypot(dx, dy);
        if (r < INNER || r > OUTER) continue;
        // Clockwise from twelve; how far behind the head, in turns.
        const angle = (Math.atan2(dx, -dy) + 2 * Math.PI) % (2 * Math.PI);
        const behind = (((head - angle) % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI) / (2 * Math.PI);
        // Brightest at the head and the band's middle, gone before a turn.
        const tone = Math.max(0, 1 - behind * 1.35) * (0.55 + 0.45 * (1 - Math.abs(r - middle) / half));
        const step = (BAYER4[(row & 3) * 4 + (col & 3)] + 0.5) / 16;
        if (Math.round(tone * 16) / 16 <= step) continue;
        const [red, green, blue] = inkAt(row, col);
        for (let y = row * cell; y < (row + 1) * cell; y++) {
          for (let x = col * cell; x < (col + 1) * cell; x++) {
            const at = (y * width + frame * side + x) * 4;
            pixels[at] = red;
            pixels[at + 1] = green;
            pixels[at + 2] = blue;
            pixels[at + 3] = 255;
          }
        }
      }
    }
  }
  const png = encodePng(width, side, pixels);
  let binary = "";
  for (let i = 0; i < png.length; i += 0x8000)
    binary += String.fromCharCode(...png.subarray(i, i + 0x8000));
  const made = { uri: `data:image/png;base64,${btoa(binary)}`, scale };
  strips.set(key, made);
  return made;
}

/** Draws the frames ahead of need, as when the drawer opens, so the cover
 *  has nothing to draw when it goes up. */
export function useWarmDitherLoading(ready: boolean) {
  useEffect(() => {
    if (ready) strip();
  }, [ready]);
}

/**
 * Fills its parent while a screen's content loads, the ring in its middle.
 * With `leaving` it plays its way out, the ring then the ground, and calls
 * `onGone` once it is clear, for the parent to let it go; asked to stay again
 * meanwhile, it comes back.
 */
export function DitherLoading({
  leaving = false,
  onGone,
}: {
  leaving?: boolean;
  onGone?: () => void;
}) {
  const colors = useColors();
  const reduced = useReducedMotion();
  const source = strip();
  const turn = useSharedValue(0);
  const ground = useSharedValue(0);
  const ring = useSharedValue(0);
  useEffect(() => {
    if (!reduced)
      turn.set(withRepeat(withTiming(FRAMES, { duration: TURN_MS, easing: Easing.linear }), -1));
  }, [turn, reduced]);
  useEffect(() => {
    if (!leaving) {
      ground.set(withTiming(1, GROUND_IN));
      ring.set(withDelay(RING_IN.delay, withTiming(1, RING_IN)));
      return;
    }
    ring.set(withTiming(0, RING_OUT));
    ground.set(
      withDelay(
        GROUND_OUT.delay,
        withTiming(0, GROUND_OUT, (finished) => {
          if (finished && onGone) scheduleOnRN(onGone);
          if (finished) scheduleOnRN(traceSwitch, "cover gone");
        }),
      ),
    );
  }, [leaving, ground, ring, onGone]);
  const groundStyle = useAnimatedStyle(() => ({ opacity: ground.get() }));
  const ringStyle = useAnimatedStyle(() => ({
    opacity: ring.get(),
    transform: [{ scale: RING_GONE_SCALE + (1 - RING_GONE_SCALE) * ring.get() }],
  }));
  const frameStyle = useAnimatedStyle(() => ({
    // A whole frame at a time, so the cells never sit between pixels.
    transform: [{ translateX: -(Math.floor(turn.get()) % FRAMES) * SIZE }],
  }));
  return (
    <Animated.View
      accessibilityLabel="Loading"
      pointerEvents="none"
      style={[styles.fill, { backgroundColor: colors.background }, groundStyle]}
    >
      <Animated.View style={[styles.window, ringStyle]}>
        <Animated.View style={[styles.strip, frameStyle]}>
          <Image
            source={{
              uri: source.uri,
              width: SIZE * FRAMES * source.scale,
              height: SIZE * source.scale,
              scale: source.scale,
            }}
            style={styles.strip}
          />
        </Animated.View>
      </Animated.View>
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  fill: {
    ...StyleSheet.absoluteFill,
    alignItems: "center",
    justifyContent: "center",
  },
  window: {
    width: SIZE,
    height: SIZE,
    overflow: "hidden",
  },
  strip: {
    width: SIZE * FRAMES,
    height: SIZE,
  },
});
