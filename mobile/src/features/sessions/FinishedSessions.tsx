import * as Haptics from "expo-haptics";
import { useEffect, useMemo, useRef, useState } from "react";
import { Pressable, Text, View } from "react-native";
import Animated, {
  Easing,
  interpolate,
  useAnimatedStyle,
  useSharedValue,
  withSpring,
  withTiming,
  ZoomOut,
  type SharedValue,
} from "react-native-reanimated";

import { relativeTime } from "@/features/workspace/relativeTime";
import type { SessionSummary } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import {
  GlassPresence,
  LiquidView,
  useGlassEntrance,
  usePopStyle,
} from "@/ui/LiquidView";
import { GLASS_MIN_OPACITY } from "@/ui/SceneFade";
import { radii, themedStyles, useColors } from "@/ui/theme";

import { ProviderLogo } from "./ProviderLogo";
import {
  dismissAllFinished,
  dismissFinished,
  type FinishedSession,
} from "./activityStore";

const CARD = 48;
/** How far each card behind the top one peeks out above it. */
const PEEK = 7;
/** Cards that peek out from behind the top one; the rest hide behind them. */
const PEEKING = 2;
const GAP = 8;
/** The row over the spread-out cards, with the button to clear them. */
const HEADER = 30;
const PRESENCE_MS = 280;
const SPRING = { damping: 18, stiffness: 180, mass: 0.9 };

/** A finished session lifting out of the pill below onto the deck. */
const LIFT_MS = 560;
const LIFT_FROM = 56;

/**
 * Sessions that finished since the app saw them working, as a deck of cards
 * over the now-playing pill, newest on top with the next two peeking out
 * behind it. Each one lifts out of the pill as it finishes. With one, a tap
 * opens it; with more, a tap spreads them out, and a tap on one then opens
 * it. A long press puts one away. Opening a session takes it off the deck.
 */
export function FinishedSessions({
  entries: given,
  hidden,
  onOpen,
}: {
  entries: FinishedSession[];
  hidden: boolean;
  onOpen: (session: SessionSummary) => void;
}) {
  const styles = useStyles();
  const colors = useColors();
  const { rememberSession, edited } = useWorkspace();
  const [open, setOpen] = useState(false);
  // As deleted or renamed here, ahead of the desktop.
  const entries = useMemo(
    () =>
      given.flatMap((entry) => {
        const session = edited(entry.session);
        if (!session) return [];
        return [session === entry.session ? entry : { ...entry, session }];
      }),
    [given, edited],
  );
  const count = entries.length;
  if (open && count < 2) setOpen(false);

  const spread = useSharedValue(0);
  useEffect(() => {
    spread.set(withSpring(open ? 1 : 0, SPRING));
  }, [open, spread]);
  // Its height eases along with the cards, so the pill stays put under it.
  const deckHeight = useSharedValue(collapsedHeight(count));
  useEffect(() => {
    deckHeight.set(
      withSpring(open ? spreadHeight(count) : collapsedHeight(count), SPRING),
    );
  }, [open, count, deckHeight]);
  const deckStyle = useAnimatedStyle(() => ({ height: deckHeight.get() }));
  const headerStyle = useAnimatedStyle(() => ({
    opacity: spread.get(),
    transform: [{ translateY: (1 - spread.get()) * 10 }],
  }));

  // A session landing on the deck taps once, as a delivery, and lifts in.
  // Those already on it when the deck mounts (back from a swipe, say) are
  // just there.
  const [onMount] = useState(() => new Set(entries.map((entry) => entry.session.id)));
  const known = useRef(onMount);
  useEffect(() => {
    const landed = entries.some((entry) => !known.current.has(entry.session.id));
    known.current = new Set(entries.map((entry) => entry.session.id));
    if (landed) void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
  }, [entries]);

  // Glass doesn't fade with its view, so it forms and clears on its own as
  // the deck comes and goes.
  const { mounted, visible, presence } = useGlassEntrance(
    count > 0 && !hidden,
    PRESENCE_MS,
  );
  const appearStyle = usePopStyle(visible, PRESENCE_MS);

  if (!mounted) return null;

  const openSession = (session: SessionSummary) => {
    setOpen(false);
    dismissFinished(session.id);
    rememberSession(session);
    onOpen(session);
  };

  return (
    <GlassPresence value={presence}>
      <Animated.View
        style={[styles.deck, deckStyle, appearStyle]}
        pointerEvents={visible ? "box-none" : "none"}
      >
        <Animated.View
          style={[styles.header, headerStyle]}
          pointerEvents={open ? "box-none" : "none"}
        >
          <Text style={styles.headerLabel}>Finished</Text>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Clear finished sessions"
            hitSlop={8}
            onPress={() => {
              void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
              if (entries[0]) dismissAllFinished(entries[0].session.projectId);
            }}
            style={({ pressed }) => [styles.clear, pressed && styles.pressed]}
          >
            <Text style={styles.clearLabel}>Clear</Text>
          </Pressable>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Stack finished sessions"
            hitSlop={8}
            onPress={() => setOpen(false)}
            style={({ pressed }) => [styles.clear, pressed && styles.pressed]}
          >
            <Icon name="chevronDown" size={14} color={colors.muted} />
          </Pressable>
        </Animated.View>
        {entries.map((entry, index) => (
          <Card
            key={entry.session.id}
            entry={entry}
            landed={!onMount.has(entry.session.id)}
            index={index}
            count={count}
            spread={spread}
            open={open}
            onPress={() => {
              if (!open && count > 1) {
                void Haptics.selectionAsync();
                setOpen(true);
              } else openSession(entry.session);
            }}
            onLongPress={() => {
              void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium);
              dismissFinished(entry.session.id);
            }}
          />
        ))}
      </Animated.View>
    </GlassPresence>
  );
}

function collapsedHeight(count: number) {
  return count === 0 ? 0 : CARD + PEEK * Math.min(count - 1, PEEKING);
}

function spreadHeight(count: number) {
  return HEADER + count * (CARD + GAP) - GAP;
}

/**
 * One finished session: its agent's mark with a check on it, its title, and
 * how long ago it finished. Stacked, only the top card's face shows; the
 * others peek out behind, a step smaller each.
 */
function Card({
  entry,
  landed,
  index,
  count,
  spread,
  open,
  onPress,
  onLongPress,
}: {
  entry: FinishedSession;
  /** It finished while the deck was up, so it lifts in from the pill. */
  landed: boolean;
  index: number;
  count: number;
  spread: SharedValue<number>;
  open: boolean;
  onPress: () => void;
  onLongPress: () => void;
}) {
  const styles = useStyles();
  const colors = useColors();
  const { session } = entry;
  // Springs to its new place as cards land on top of it or leave.
  const slot = useSharedValue(index);
  useEffect(() => {
    slot.set(withSpring(index, SPRING));
  }, [index, slot]);
  const place = useAnimatedStyle(() => {
    const at = slot.get();
    const p = spread.get();
    const behind = Math.min(at, PEEKING);
    return {
      zIndex: count - index,
      transform: [
        {
          translateY: interpolate(p, [0, 1], [-PEEK * behind, -at * (CARD + GAP)]),
        },
        { scale: interpolate(p, [0, 1], [1 - 0.05 * behind, 1]) },
      ],
    };
  });
  // Lifts out of the pill as it lands. Run by hand rather than as an
  // `entering` animation, which is skipped when the deck mounts with it, as
  // it does for the first session to finish.
  const lift = useSharedValue(landed ? 0 : 1);
  useEffect(() => {
    if (lift.get() === 1) return;
    lift.set(
      withTiming(1, { duration: LIFT_MS, easing: Easing.out(Easing.back(1.4)) }),
    );
    // Only as it mounts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const liftStyle = useAnimatedStyle(() => {
    const t = lift.get();
    return {
      // Never quite clear, or the card's glass comes up without its effect.
      opacity: Math.max(GLASS_MIN_OPACITY, Math.min(1, t / 0.45)),
      transform: [{ translateY: (1 - t) * LIFT_FROM }, { scale: 0.7 + 0.3 * t }],
    };
  });
  // Behind the top card, only the glass edge shows.
  const face = useAnimatedStyle(() => ({
    opacity: Math.max(spread.get(), Math.max(0, 1 - slot.get())),
  }));

  return (
    <Animated.View style={[styles.cardSlot, place]} pointerEvents="box-none">
      <Animated.View style={liftStyle} exiting={ZoomOut.duration(200)}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={
            !open && count > 1
              ? `${count} finished sessions. Show them`
              : `${session.title}, finished. Open`
          }
          accessibilityActions={[{ name: "longpress", label: "Put away" }]}
          onAccessibilityAction={({ nativeEvent }) => {
            if (nativeEvent.actionName === "longpress") onLongPress();
          }}
          onPress={onPress}
          onLongPress={onLongPress}
          // Only the top card takes taps while stacked.
          disabled={!open && index > 0}
        >
          <LiquidView borderRadius={radii.pill} innerStyle={styles.card}>
            <Animated.View style={[styles.cardFace, face]}>
              <View style={styles.cardArt}>
                <ProviderLogo harness={session.harness} size={16} color={colors.content} />
                <View style={styles.check}>
                  <Icon name="check" size={9} color={colors.inverse} strokeWidth={3} />
                </View>
              </View>
              <Text style={styles.cardTitle} numberOfLines={1}>
                {session.title}
              </Text>
              {!open && count > 1 ? (
                <View style={styles.more}>
                  <Text style={styles.moreLabel}>{`+${count - 1}`}</Text>
                </View>
              ) : (
                <Text style={styles.time}>{relativeTime(entry.at)}</Text>
              )}
            </Animated.View>
          </LiquidView>
        </Pressable>
      </Animated.View>
    </Animated.View>
  );
}

const useStyles = themedStyles((colors) => ({
  deck: {
    marginHorizontal: 28,
  },
  header: {
    position: "absolute",
    top: 0,
    left: 4,
    right: 0,
    height: HEADER - GAP,
    flexDirection: "row",
    alignItems: "center",
    gap: 4,
  },
  headerLabel: {
    flex: 1,
    color: colors.muted,
    fontSize: 13,
    fontWeight: "600",
  },
  clear: {
    height: 22,
    minWidth: 22,
    paddingHorizontal: 8,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 11,
    backgroundColor: colors.fill,
  },
  clearLabel: {
    color: colors.muted,
    fontSize: 12,
    fontWeight: "600",
  },
  pressed: {
    opacity: 0.6,
  },
  cardSlot: {
    position: "absolute",
    left: 0,
    right: 0,
    bottom: 0,
  },
  card: {
    height: CARD,
    paddingHorizontal: 8,
  },
  cardFace: {
    flex: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
  },
  cardArt: {
    width: 32,
    height: 32,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 16,
    backgroundColor: colors.fill,
  },
  check: {
    position: "absolute",
    right: -2,
    bottom: -2,
    width: 14,
    height: 14,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 7,
    backgroundColor: colors.additions,
  },
  cardTitle: {
    flex: 1,
    color: colors.content,
    fontSize: 14,
    fontWeight: "600",
  },
  time: {
    marginRight: 6,
    color: colors.faint,
    fontSize: 12,
    fontVariant: ["tabular-nums"],
  },
  more: {
    marginRight: 2,
    paddingHorizontal: 8,
    height: 22,
    justifyContent: "center",
    borderRadius: 11,
    backgroundColor: colors.fill,
  },
  moreLabel: {
    color: colors.muted,
    fontSize: 12,
    fontWeight: "600",
    fontVariant: ["tabular-nums"],
  },
}));
