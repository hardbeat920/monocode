import { useEffect, useMemo, useState } from "react";
import { StyleSheet, Text, View, type LayoutChangeEvent } from "react-native";
import { useReducedMotion } from "react-native-reanimated";

import { MONO } from "@/features/changes/diffTheme";

/** Characters from empty to dense. */
const RAMP = " .·:-=+*#%@";

const FONT_SIZE = 10;
const LINE_HEIGHT = 12;
/** Menlo's advance at FONT_SIZE. */
const CHAR_WIDTH = FONT_SIZE * 0.602;

/**
 * The colours the field sweeps through, in order and back to the first: the
 * dark theme's accent, then violet, pink, amber and its additions' mint.
 */
const HUES = ["#4c9ef5", "#a78bfa", "#f472b6", "#fbbf24", "#6ee7b7"];

/** Frames a second: enough to drift, few enough to stay cheap. */
const FPS = 10;

/** How far down the field reaches, as a share of the height, before it's gone. */
const REACH = 0.55;

/**
 * Interfering waves drawn in characters, drifting slowly: densest at the top
 * and thinning out before halfway down, so the words and buttons at the foot
 * sit on plain black. Coloured in bands that sweep diagonally across it.
 * Still, on one frame, when motion is reduced or it's asked to be.
 */
export function AsciiField({
  opacity = 0.3,
  still = false,
}: {
  opacity?: number;
  /** Drawn once and left, as behind an empty session. */
  still?: boolean;
}) {
  const reduceMotion = useReducedMotion();
  const [size, setSize] = useState<{ cols: number; rows: number } | null>(null);
  const [time, setTime] = useState(0);

  useEffect(() => {
    if (reduceMotion || still || !size) return;
    const started = Date.now() - time * 1000;
    const interval = setInterval(
      () => setTime((Date.now() - started) / 1000),
      1000 / FPS,
    );
    return () => clearInterval(interval);
    // `time` only seeds the clock, so a restart picks up where it left off.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reduceMotion, still, size]);

  // Worked out again only for a new frame, not whenever the parent renders.
  const runs = useMemo(
    () => (size ? frame(size.cols, size.rows, time) : []),
    [size, time],
  );

  const onLayout = (event: LayoutChangeEvent) => {
    const { width, height } = event.nativeEvent.layout;
    setSize({
      cols: Math.ceil(width / CHAR_WIDTH),
      rows: Math.ceil((height * REACH) / LINE_HEIGHT),
    });
  };

  return (
    <View pointerEvents="none" style={styles.root} onLayout={onLayout}>
      {size ? (
        <Text
          // A little wider than the screen, so no line wraps; the rest is clipped.
          style={[
            styles.field,
            { opacity, width: (size.cols + 2) * CHAR_WIDTH },
          ]}
          accessible={false}
          allowFontScaling={false}
        >
          {runs.map((run, index) => (
            <Text key={index} style={{ color: HUES[run.hue] }}>
              {run.text}
            </Text>
          ))}
        </Text>
      ) : null}
    </View>
  );
}

/** A stretch of characters in one colour, as an index into HUES. */
type Run = { hue: number; text: string };

/**
 * One frame, as runs of a colour each, so the text is a few hundred spans
 * rather than one per character.
 */
function frame(cols: number, rows: number, t: number) {
  const runs: Run[] = [];
  const add = (hue: number, char: string) => {
    const last = runs[runs.length - 1];
    // Blanks take any colour, so they join the run before them.
    if (last && (last.hue === hue || char === " " || char === "\n"))
      last.text += char;
    else runs.push({ hue, text: char });
  };
  // Ripples spread from just above the mark, at the top centre.
  const cx = cols / 2;
  const cy = -rows * 0.1;
  for (let y = 0; y < rows; y++) {
    // Fades out down the field, eased so it lingers near the top.
    const fade = (1 - y / rows) ** 1.6;
    if (y > 0) add(0, "\n");
    for (let x = 0; x < cols; x++) {
      // Cells are about twice as tall as wide, so y counts double.
      const dx = x - cx;
      const dy = (y - cy) * 2;
      const wave =
        Math.sin(x * 0.11 + t * 0.6) +
        Math.sin(y * 0.23 - t * 0.45) +
        Math.sin((x + y * 2) * 0.07 + t * 0.3) +
        Math.sin(Math.sqrt(dx * dx + dy * dy) * 0.16 - t * 0.9) +
        // A finer ripple, for grain.
        Math.sin(x * 0.42 - y * 0.6 + t * 1.3) * 0.6;
      // From -4.6..4.6 to 0..1, then shaped and faded.
      const level = ((wave + 4.6) / 9.2) ** 1.4 * fade;
      const char =
        RAMP[Math.min(RAMP.length - 1, Math.floor(level * RAMP.length))];
      // Bands running diagonally, nudged by the waves and drifting with time.
      const sweep = (x * 0.012 + y * 0.025 + wave * 0.05 - t * 0.04) % 1;
      add(Math.floor((sweep + 1) % 1 * HUES.length), char);
    }
  }
  return runs;
}

const styles = StyleSheet.create({
  root: {
    position: "absolute",
    top: 0,
    right: 0,
    bottom: 0,
    left: 0,
    overflow: "hidden",
  },
  field: {
    fontFamily: MONO,
    fontSize: FONT_SIZE,
    lineHeight: LINE_HEIGHT,
  },
});
