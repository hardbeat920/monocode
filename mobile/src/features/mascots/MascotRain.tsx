import { Accelerometer } from "expo-sensors";
import { memo, useEffect, useState } from "react";
import { StyleSheet, useWindowDimensions, View } from "react-native";
import Animated, {
  useAnimatedStyle,
  useFrameCallback,
  useReducedMotion,
  useSharedValue,
  type SharedValue,
} from "react-native-reanimated";

import { PROJECT_COLORS } from "@/features/workspace/projectColors";

import { step, screenGravity, type Body, type Collider } from "./mascotPhysics";
import { PIXEL_MASCOT_NAMES } from "./pixelMascots";
import { PixelMascot } from "./PixelMascot";

const COUNT = 18;
const SIZE = { min: 26, max: 36 };
/** The sprite fills the middle 12 of its 16-unit grid; the circle hugs that. */
const RADIUS = 0.36;
/** Seconds over which they are let go, one by one. */
const DROP_SPREAD = 2.6;
/** Milliseconds between readings; the world steps with the frames between them. */
const SENSOR_INTERVAL = 16;

type Mascot = { name: string; color: string; size: number };

const random = (min: number, max: number) => min + Math.random() * (max - min);

/** The cast, and where each starts above the screen, mixed afresh each time. */
function cast(width: number) {
  const colors = PROJECT_COLORS.slice(1);
  const mascots: Mascot[] = [];
  const bodies: Body[] = [];
  for (let i = 0; i < COUNT; i++) {
    const size = Math.round(random(SIZE.min, SIZE.max));
    const radius = size * RADIUS;
    mascots.push({
      name: PIXEL_MASCOT_NAMES[i % PIXEL_MASCOT_NAMES.length],
      color: colors[Math.floor(Math.random() * colors.length)],
      size,
    });
    bodies.push({
      x: random(radius, width - radius),
      y: -size - random(0, 120),
      vx: random(-60, 60),
      vy: random(0, 200),
      angle: random(-0.6, 0.6),
      spin: random(-3, 3),
      radius,
      // Uneven: bunched toward the start, with stragglers.
      dropAt: DROP_SPREAD * Math.random() ** 1.6,
      dropped: false,
      entered: false,
    });
  }
  return { mascots, bodies };
}

/**
 * Monocode's mascots raining down from the top of the screen, piling on
 * `colliders` and on the screen's foot, and sliding about as the phone tilts.
 * Fills its parent, which should be the whole screen; touches pass through.
 * Nothing shows with Reduce Motion on.
 */
export function MascotRain({ colliders }: { colliders: SharedValue<Collider[]> }) {
  const reduceMotion = useReducedMotion();
  if (reduceMotion) return null;
  return <Rain colliders={colliders} />;
}

function Rain({ colliders }: { colliders: SharedValue<Collider[]> }) {
  const { width, height } = useWindowDimensions();
  const [{ mascots, bodies: start }] = useState(() => cast(width));
  const bodies = useSharedValue(start);
  // Straight down until the accelerometer says otherwise, as in the simulator.
  const gravity = useSharedValue({ x: 0, y: 1 });
  const time = useSharedValue(0);

  useEffect(() => {
    let subscription: { remove: () => void } | null = null;
    let cancelled = false;
    void Accelerometer.isAvailableAsync().then((available) => {
      if (!available || cancelled) return;
      Accelerometer.setUpdateInterval(SENSOR_INTERVAL);
      subscription = Accelerometer.addListener(({ x, y }) => {
        gravity.set(screenGravity(x, y));
      });
    });
    return () => {
      cancelled = true;
      subscription?.remove();
    };
  }, [gravity]);

  useFrameCallback(({ timeSincePreviousFrame }) => {
    // Held short after a stall, so nothing tunnels through a button.
    const dt = Math.min((timeSincePreviousFrame ?? 16) / 1000, 1 / 30);
    time.set(time.get() + dt);
    const now = time.get();
    const world = colliders.get();
    const pull = gravity.get();
    bodies.modify((list) => {
      "worklet";
      step(list, world, pull, width, height, dt, now);
      return list;
    });
  });

  return (
    <View pointerEvents="none" style={StyleSheet.absoluteFill}>
      {mascots.map((mascot, index) => (
        <Falling key={index} index={index} mascot={mascot} bodies={bodies} />
      ))}
    </View>
  );
}

function Falling({
  index,
  mascot,
  bodies,
}: {
  index: number;
  mascot: Mascot;
  bodies: SharedValue<Body[]>;
}) {
  const half = mascot.size / 2;
  const style = useAnimatedStyle(() => {
    const body = bodies.get()[index];
    return {
      transform: [
        { translateX: body.x - half },
        { translateY: body.y - half },
        { rotate: `${body.angle}rad` },
      ],
    };
  });
  return (
    <Animated.View style={[styles.mascot, { width: mascot.size, height: mascot.size }, style]}>
      <Sprite {...mascot} />
    </Animated.View>
  );
}

/** Drawn once and kept as a bitmap, as it only ever moves and turns. */
const Sprite = memo(function Sprite({ name, color, size }: Mascot) {
  return (
    <View shouldRasterizeIOS renderToHardwareTextureAndroid>
      <PixelMascot name={name} color={color} size={size} />
    </View>
  );
});

const styles = StyleSheet.create({
  mascot: {
    position: "absolute",
    left: 0,
    top: 0,
  },
});
