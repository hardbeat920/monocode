import MaskedView from "@react-native-masked-view/masked-view";
import { LinearGradient } from "expo-linear-gradient";
import {
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type Ref,
} from "react";
import {
  StyleSheet,
  Text,
  View,
  type StyleProp,
  type TextStyle,
} from "react-native";
import Animated, {
  Easing,
  FadeIn,
  FadeOut,
  useAnimatedReaction,
  useAnimatedStyle,
  useFrameCallback,
  useReducedMotion,
  useSharedValue,
  withTiming,
  type SharedValue,
} from "react-native-reanimated";
import { scheduleOnRN } from "react-native-worklets";

/** Points a second, about the pace of the Music app's now-playing title. */
const SPEED = 30;
/** Space between one title and the next. */
const GAP = 56;
/** How far the edges fade into the surface. */
const FADE = 16;
/** A title is the one in front once its start is this close to the left edge. */
const LEAD = FADE + 6;
/** How long a skip takes to bring the next title in. */
const SKIP_MS = 520;
/** Wide enough for every title, so none wraps. */
const TRACK = 8000;

export type TickerItem = { key: string; text: string };

export type TickerHandle = {
  /** Brings an item to the front, sliding the line along to it. */
  skipTo: (index: number) => void;
};

/**
 * Titles on one line scrolling left without end, as the Music app's
 * now-playing title, edges fading into the surface: one after another, then
 * round again. `onActiveChange` says which is in front, the one whose start
 * last reached the left edge. A single title that fits stays still. With
 * Reduce Motion on, only the one in front shows, cut short, and a skip
 * fades the next one in.
 */
export function Ticker({
  items,
  style,
  onActiveChange,
  held,
  ref,
}: {
  items: TickerItem[];
  style: StyleProp<TextStyle>;
  onActiveChange: (index: number) => void;
  /** Holds the line still while true, say as the screen under it slides, so
   *  the two don't both draw every frame. */
  held?: SharedValue<boolean>;
  ref?: Ref<TickerHandle>;
}) {
  const reduceMotion = useReducedMotion();
  const [box, setBox] = useState(0);
  const [widths, setWidths] = useState<Record<string, number>>({});
  // A title joins the line once measured, so a new one doesn't hold it up.
  const line = items.filter((item) => widths[item.key] !== undefined);
  const starts: number[] = [];
  let total = 0;
  for (const item of line) {
    starts.push(total);
    total += widths[item.key] + GAP;
  }
  const moving =
    !reduceMotion &&
    box > 0 &&
    (line.length > 1 || (line.length === 1 && widths[line[0].key] > box));

  // How far the line has travelled, without end; it shows that modulo its length.
  const travel = useSharedValue(0);
  const length = useSharedValue(total);
  const offsets = useSharedValue(starts);
  const skipping = useSharedValue(false);
  const [still, setStill] = useState(0);

  // As titles come and go, the one in front keeps its place on screen.
  const layout = useRef<{ keys: string[]; starts: number[]; total: number } | null>(null);
  /** The one in front, by its place in the line. */
  const front = useRef(0);
  const lineKeys = line.map((item) => item.key).join("\n");
  useLayoutEffect(() => {
    const before = layout.current;
    const keys = line.map((item) => item.key);
    layout.current = { keys, starts, total };
    if (before && before.total > 0 && total > 0) {
      const key = before.keys[front.current];
      const within =
        ((travel.get() % before.total) + before.total) % before.total -
        (before.starts[front.current] ?? 0);
      const at = keys.indexOf(key);
      travel.set(
        at >= 0
          ? starts[at] + within
          : (starts[Math.min(front.current, starts.length - 1)] ?? 0),
      );
    }
    length.set(total);
    offsets.set(starts);
    // Reruns only when the line itself changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [total, lineKeys]);

  const frame = useFrameCallback(({ timeSincePreviousFrame }) => {
    if (skipping.get() || held?.get()) return;
    // Held short after a stall, so it doesn't leap.
    const dt = Math.min((timeSincePreviousFrame ?? 16) / 1000, 1 / 20);
    travel.set(travel.get() + SPEED * dt);
  }, false);
  useLayoutEffect(() => {
    frame.setActive(moving);
  }, [frame, moving]);

  // Reached through a ref, as the reaction below keeps its first closure.
  // The line's order is the items' with some not in it yet, so a place in
  // the line is told as the item's own.
  const latest = useRef({ onActiveChange, items, line });
  useLayoutEffect(() => {
    latest.current = { onActiveChange, items, line };
  });
  const [report] = useState(() => (index: number) => {
    front.current = index;
    const { onActiveChange: notify, items: all, line: shownLine } = latest.current;
    const key = shownLine[index]?.key;
    const at = all.findIndex((item) => item.key === key);
    if (at >= 0) notify(at);
  });
  useAnimatedReaction(
    () => {
      const all = length.get();
      const marks = offsets.get();
      if (all <= 0 || marks.length === 0) return 0;
      const at = (((travel.get() + LEAD) % all) + all) % all;
      let index = 0;
      for (let i = 0; i < marks.length; i++) if (marks[i] <= at) index = i;
      return index;
    },
    (index, previous) => {
      if (index !== previous && moving) scheduleOnRN(report, index);
    },
    [moving],
  );

  useImperativeHandle(ref, () => ({
    skipTo: (index) => {
      if (!moving) {
        setStill(index);
        onActiveChange(index);
        return;
      }
      // Not measured yet, it isn't on the line to skip to.
      const place = line.findIndex((item) => item.key === items[index]?.key);
      if (place < 0) return;
      const all = length.get();
      const now = travel.get();
      const at = ((now % all) + all) % all;
      let target = now - at + starts[place];
      if (target <= now + 1) target += all;
      skipping.set(true);
      travel.set(
        withTiming(
          target,
          { duration: SKIP_MS, easing: Easing.inOut(Easing.cubic) },
          () => skipping.set(false),
        ),
      );
    },
  }));

  const track = useAnimatedStyle(() => {
    const all = length.get();
    return {
      transform: [
        { translateX: all > 0 ? -(((travel.get() % all) + all) % all) : 0 },
      ],
    };
  });

  // Enough rounds of the line to fill the box while the first scrolls off.
  const rounds = moving ? Math.max(2, Math.ceil(box / total) + 1) : 0;
  const shown = items[Math.min(still, items.length - 1)];

  return (
    <View
      style={styles.box}
      onLayout={(event) => setBox(Math.floor(event.nativeEvent.layout.width))}
    >
      {/* Measures each title at its full width, unseen. */}
      <View style={styles.measure} pointerEvents="none">
        {items.map((item) => (
          <Text
            key={item.key}
            style={style}
            numberOfLines={1}
            onLayout={(event) => {
              const width = Math.ceil(event.nativeEvent.layout.width);
              setWidths((previous) =>
                previous[item.key] === width
                  ? previous
                  : { ...previous, [item.key]: width },
              );
            }}
          >
            {item.text}
          </Text>
        ))}
      </View>
      {moving ? (
        <>
          {/* Holds the box's height. */}
          <Text style={[style, styles.hidden]} numberOfLines={1}>
            {" "}
          </Text>
          <MaskedView
            style={StyleSheet.absoluteFill}
            maskElement={
              <View style={styles.mask}>
                <LinearGradient
                  colors={["transparent", "black"]}
                  start={{ x: 0, y: 0.5 }}
                  end={{ x: 1, y: 0.5 }}
                  style={styles.cap}
                />
                <View style={styles.middle} />
                <LinearGradient
                  colors={["black", "transparent"]}
                  start={{ x: 0, y: 0.5 }}
                  end={{ x: 1, y: 0.5 }}
                  style={styles.cap}
                />
              </View>
            }
          >
            {/* As wide as its titles, so the masked layer is no bigger than them. */}
            <Animated.View style={[styles.track, { width: total * rounds }, track]}>
              {Array.from({ length: rounds }, (_, round) =>
                line.map((item) => (
                  <Text
                    key={`${round}:${item.key}`}
                    style={[style, { marginRight: GAP }]}
                    numberOfLines={1}
                  >
                    {item.text}
                  </Text>
                )),
              )}
            </Animated.View>
          </MaskedView>
        </>
      ) : shown ? (
        <Animated.Text
          key={shown.key}
          style={style}
          numberOfLines={1}
          entering={FadeIn.duration(220)}
          exiting={FadeOut.duration(160)}
        >
          {shown.text}
        </Animated.Text>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create({
  box: {
    alignSelf: "stretch",
    overflow: "hidden",
  },
  measure: {
    position: "absolute",
    width: TRACK,
    flexDirection: "row",
    alignItems: "flex-start",
    opacity: 0,
  },
  hidden: {
    opacity: 0,
  },
  mask: {
    flex: 1,
    flexDirection: "row",
  },
  cap: {
    width: FADE,
  },
  middle: {
    flex: 1,
    backgroundColor: "black",
  },
  track: {
    flexDirection: "row",
  },
});
