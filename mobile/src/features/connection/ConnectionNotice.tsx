import { useEffect } from "react";
import { Text, View } from "react-native";
import Animated, {
  Easing,
  useAnimatedStyle,
  useSharedValue,
  withRepeat,
  withTiming,
} from "react-native-reanimated";

import { useSessionLayout } from "@/features/sessions/SessionScreen";
import { LiquidView } from "@/ui/LiquidView";
import { withAlpha, themedStyles, useColors } from "@/ui/theme";

import { useConnection } from "./ConnectionProvider";

/** The desktop can't be reached: a poll failed and none has since succeeded. */
export function useOffline() {
  const connection = useConnection();
  // `connected` starts false before the first poll; only a failed one counts.
  return !!connection.pairing && !connection.connected && !!connection.error;
}

/**
 * Errors other than losing the desktop, like a conversation that failed to
 * load, as a quiet line under the header. Losing the desktop shows in the
 * composer instead, and under the project's worktree line.
 */
export function ConnectionNotice({ error }: { error?: string | null }) {
  const styles = useStyles();
  const offline = useOffline();
  const { top } = useSessionLayout();
  return !offline && error ? (
    <Text style={[styles.inline, { top }]} numberOfLines={2}>
      {error}
    </Text>
  ) : null;
}

/** How long one pulse of the reconnecting dot takes. */
const PULSE_MS = 1400;

/**
 * A small capsule in place of the worktree line while the desktop can't be
 * reached: a red dot pulsing out like a recording light, and that the app
 * keeps trying. Tapping it tries again at once. Nothing when connected.
 */
export function ReconnectingStatus() {
  const colors = useColors();
  const styles = useStyles();
  const offline = useOffline();
  const connection = useConnection();
  if (!offline) return null;
  return (
    // Glass, as the header's buttons are, faintly red.
    <LiquidView
      accessibilityLabel="Reconnecting to your desktop. Tap to try again now."
      // A failed try shows as staying offline.
      onPress={() => void connection.refresh().catch(() => {})}
      pressedScale={1.04}
      tintColor={withAlpha(colors.deletions, 0.12)}
      innerStyle={styles.status}
    >
      <PulsingDot />
      <Text style={styles.statusLabel} numberOfLines={1}>
        Reconnecting to desktop
      </Text>
    </LiquidView>
  );
}

/** A red dot pulsing out like a recording light. */
export function PulsingDot() {
  const styles = useStyles();
  const pulse = useSharedValue(0);
  useEffect(() => {
    pulse.set(
      withRepeat(
        withTiming(1, { duration: PULSE_MS, easing: Easing.out(Easing.cubic) }),
        -1,
      ),
    );
  }, [pulse]);
  // A ring grows out of the dot and fades, while the dot dims and comes back.
  const ring = useAnimatedStyle(() => ({
    opacity: 0.6 * (1 - pulse.get()),
    transform: [{ scale: 1 + pulse.get() * 1.6 }],
  }));
  const dot = useAnimatedStyle(() => ({
    opacity: 0.55 + 0.45 * Math.abs(Math.cos(pulse.get() * Math.PI)),
  }));
  return (
    <View style={styles.dotBox}>
      <Animated.View style={[styles.dot, ring]} />
      <Animated.View style={[styles.dot, dot]} />
    </View>
  );
}

const DOT = 7;

const useStyles = themedStyles((colors) => ({
  // Exactly the 22pt line under the header it stands in for, which would
  // otherwise cut a taller pill off.
  status: {
    height: 22,
    gap: 7,
    paddingLeft: 9,
    paddingRight: 11,
  },
  statusLabel: {
    color: colors.deletions,
    fontSize: 12,
    lineHeight: 15,
    fontWeight: "600",
  },
  dotBox: {
    width: DOT,
    height: DOT,
  },
  dot: {
    position: "absolute",
    width: DOT,
    height: DOT,
    borderRadius: DOT / 2,
    backgroundColor: colors.deletions,
  },
  inline: {
    position: "absolute",
    zIndex: 5,
    left: 20,
    right: 20,
    padding: 10,
    color: colors.muted,
    fontSize: 13,
  },
}));
