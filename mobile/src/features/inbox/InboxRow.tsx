import * as Haptics from "expo-haptics";
import { memo, useRef } from "react";
import { Pressable, Text, View } from "react-native";
import ReanimatedSwipeable, {
  type SwipeableMethods,
} from "react-native-gesture-handler/ReanimatedSwipeable";
import Animated, {
  interpolate,
  useAnimatedReaction,
  useAnimatedStyle,
  useSharedValue,
  type EntryOrExitLayoutType,
  type SharedValue,
} from "react-native-reanimated";
import { scheduleOnRN } from "react-native-worklets";

import { relativeTime } from "@/features/workspace/relativeTime";
import { Icon, type IconName } from "@/ui/Icon";
import { ChoiceMenu } from "@/ui/ChoiceMenu";
import { radii, themedStyles, useColors, withAlpha } from "@/ui/theme";

import { snoozeOptions } from "./model";
import { ProviderMark } from "./ProviderMark";
import type { InboxItem } from "./protocol";

const ACTION_SIZE = 40;
const ACTION_GAP = 8;
/** Three buttons and the gaps around them: how far the row slides to show them. */
const ACTIONS_WIDTH = ACTION_SIZE * 3 + ACTION_GAP * 3 + 4;
/** How much farther past the buttons a swipe commits Done. */
const COMMIT_PAST = 64;

export type RowHandlers = {
  open: (item: InboxItem) => void;
  done: (item: InboxItem) => void;
  undone: (item: InboxItem) => void;
  snooze: (item: InboxItem, until: number) => void;
  startAgent: (item: InboxItem) => void;
  /** Told when a row's actions open, to close another row's. */
  swipeOpened: (row: SwipeableMethods) => void;
};

/** The words under a row's title: why it is here. */
export function whyLabel(item: InboxItem) {
  if (item.attentionLabel) return item.attentionLabel;
  switch (item.status) {
    case "merged":
      return "Merged";
    case "closed":
      return "Closed";
    case "draft":
      return "Draft";
    default:
      return item.kind === "pr" ? "Open pull request" : "Open issue";
  }
}

/**
 * An inbox item as the mock lays it out: the provider's mark, then the title
 * with its age, why it's here, and where it lives with its size, comments and
 * checks. Swiping it left uncovers Snooze, Start agent and Done, as in Mail;
 * swiped all the way, it commits Done. Right swipes are left to the page.
 */
export const InboxRow = memo(function InboxRow({
  item,
  handlers,
  now,
  exiting,
}: {
  item: InboxItem;
  handlers: RowHandlers;
  now: number;
  exiting?: EntryOrExitLayoutType;
}) {
  const colors = useColors();
  const styles = useStyles();
  const swipeable = useRef<SwipeableMethods>(null);
  const opened = useRef(false);
  const armed = useSharedValue(false);

  const tone =
    item.attention === "ciFailed"
      ? colors.deletions
      : item.attention && item.attention !== "other"
        ? colors.caution
        : colors.muted;

  const close = () => swipeable.current?.close();
  const act = (action: () => void) => () => {
    close();
    action();
  };
  const commitDone = () => {
    void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
    handlers.done(item);
  };

  const summary = [
    item.unread ? "Unread" : null,
    item.title,
    whyLabel(item),
    item.repo,
  ]
    .filter(Boolean)
    .join(", ");

  return (
    <Animated.View exiting={exiting}>
      <ReanimatedSwipeable
        ref={swipeable}
        friction={1}
        rightThreshold={ACTION_SIZE}
        overshootRight
        overshootFriction={4}
        dragOffsetFromRightEdge={10}
        // Its actions are on the right, so only a swipe left is its own; one
        // right goes to the page, which swipes back a page or opens the drawer.
        dragOffsetFromLeftEdge={Number.MAX_SAFE_INTEGER}
        onSwipeableOpenStartDrag={() => {
          armed.set(false);
        }}
        onSwipeableWillOpen={() => {
          opened.current = true;
          // Let go past the buttons: that is Done, committed.
          if (armed.get() && !item.done) {
            armed.set(false);
            close();
            commitDone();
            return;
          }
          if (swipeable.current) handlers.swipeOpened(swipeable.current);
        }}
        onSwipeableWillClose={() => {
          opened.current = false;
        }}
        renderRightActions={(progress, translation) => (
          <RowActions
            progress={progress}
            translation={translation}
            armed={armed}
            item={item}
            onUndone={act(() => handlers.undone(item))}
            onDone={act(commitDone)}
            onStart={act(() => handlers.startAgent(item))}
            onSnooze={(until) => {
              close();
              handlers.snooze(item, until);
            }}
            now={now}
          />
        )}
      >
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={summary}
          accessibilityActions={[
            { name: "done", label: item.done ? "Move back to inbox" : "Mark done" },
            { name: "agent", label: "Start agent" },
          ]}
          onAccessibilityAction={({ nativeEvent }) => {
            if (nativeEvent.actionName === "done") {
              if (item.done) handlers.undone(item);
              else commitDone();
            }
            if (nativeEvent.actionName === "agent") handlers.startAgent(item);
          }}
          // A tap on a row with its actions out puts them away instead.
          onPress={() => (opened.current ? close() : handlers.open(item))}
          style={({ pressed }) => [styles.row, pressed && styles.pressed]}
        >
          <View pointerEvents="none" style={[styles.dot, !item.unread && styles.dotRead]} />
          <ProviderMark source={item.source} />
          <View style={styles.body}>
            <View style={styles.line}>
              <Text
                numberOfLines={1}
                style={[styles.title, !item.unread && styles.titleRead]}
              >
                {item.title}
              </Text>
              <Text style={[styles.time, item.unread && styles.timeUnread]}>
                {relativeTime(item.updatedAt, now)}
              </Text>
            </View>
            <Text numberOfLines={1} style={[styles.why, { color: tone }]}>
              {item.snoozedUntil && item.snoozedUntil > now
                ? `Snoozed until ${snoozedLabel(item.snoozedUntil, now)}`
                : whyLabel(item)}
            </Text>
            <View style={styles.line}>
              <Text numberOfLines={1} style={styles.ref}>
                {[item.repo, item.number].filter(Boolean).join(" · ")}
              </Text>
              <View style={styles.stats}>
                {item.kind === "pr" && item.additions !== undefined ? (
                  <>
                    <Text style={[styles.stat, { color: colors.additions }]}>+{item.additions}</Text>
                    <Text style={[styles.stat, { color: colors.deletions }]}>
                      −{item.deletions ?? 0}
                    </Text>
                  </>
                ) : null}
                {item.comments > 0 ? (
                  <View style={styles.count}>
                    <Icon name="comment" size={14} color={colors.faint} />
                    <Text style={styles.stat}>{item.comments}</Text>
                  </View>
                ) : null}
                {item.checks ? <ChecksGlyph state={item.checks} /> : null}
              </View>
            </View>
          </View>
        </Pressable>
      </ReanimatedSwipeable>
    </Animated.View>
  );
});

/** Tomorrow-style label for a snooze: the time today, else the weekday and time. */
export function snoozedLabel(until: number, now: number) {
  const date = new Date(until);
  const time = date.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  const sameDay = new Date(now).toDateString() === date.toDateString();
  return sameDay
    ? time
    : `${date.toLocaleDateString(undefined, { weekday: "short" })}, ${time}`;
}

export function ChecksGlyph({ state }: { state: NonNullable<InboxItem["checks"]> }) {
  const colors = useColors();
  if (state === "passing") return <Icon name="passing" size={14} color={colors.additions} />;
  if (state === "failing") return <Icon name="failing" size={14} color={colors.deletions} />;
  return <Icon name="clock" size={14} color={colors.caution} />;
}

function RowActions({
  progress,
  translation,
  armed,
  item,
  now,
  onUndone,
  onDone,
  onStart,
  onSnooze,
}: {
  progress: SharedValue<number>;
  translation: SharedValue<number>;
  armed: SharedValue<boolean>;
  item: InboxItem;
  now: number;
  onUndone: () => void;
  onDone: () => void;
  onStart: () => void;
  onSnooze: (until: number) => void;
}) {
  const colors = useColors();
  const styles = useStyles();

  // Past the buttons by enough, a release is Done: tick once as it arms, and
  // again if the finger comes back.
  useAnimatedReaction(
    () => -translation.get() > ACTIONS_WIDTH + COMMIT_PAST && !item.done,
    (isArmed, was) => {
      armed.set(isArmed);
      if (isArmed !== was && was !== null) scheduleOnRN(Haptics.selectionAsync);
    },
    [item.done],
  );
  return (
    <View style={styles.actions}>
      {item.done ? (
        <RowAction
          progress={progress}
          from={0}
          icon="undo"
          label="Back to inbox"
          color={colors.content}
          background={colors.surfaceRaised}
          onPress={onUndone}
        />
      ) : (
        <ChoiceMenu
          label="Snooze until"
          actions={snoozeOptions(now).map((option) => ({
            label: option.label,
            onPress: () => onSnooze(option.until),
          }))}
        >
          <ActionFace
            progress={progress}
            from={0.5}
            icon="clock"
            color={colors.content}
            background={colors.surfaceRaised}
          />
        </ChoiceMenu>
      )}
      <RowAction
        progress={progress}
        from={0.25}
        icon="sparkle"
        label="Start agent"
        color={colors.accent}
        background={withAlpha(colors.accent, 0.18)}
        onPress={onStart}
      />
      {item.done ? null : (
        <RowAction
          progress={progress}
          from={0}
          icon="check"
          label="Done"
          color={colors.additions}
          background={withAlpha(colors.additions, 0.18)}
          onPress={onDone}
        />
      )}
    </View>
  );
}

function ActionFace({
  progress,
  from,
  icon,
  color,
  background,
}: {
  progress: SharedValue<number>;
  from: number;
  icon: IconName;
  color: string;
  background: string;
}) {
  const styles = useStyles();
  const style = useAnimatedStyle(() => {
    const shown = interpolate(progress.get(), [from, from + 0.65], [0, 1], "clamp");
    return { opacity: shown, transform: [{ scale: 0.7 + 0.3 * shown }] };
  });
  return (
    <Animated.View
      style={[styles.action, { backgroundColor: background }, style]}
    >
      <Icon name={icon} size={18} color={color} />
    </Animated.View>
  );
}

function RowAction({
  progress,
  from,
  icon,
  label,
  color,
  background,
  onPress,
}: {
  progress: SharedValue<number>;
  from: number;
  icon: IconName;
  label: string;
  color: string;
  background: string;
  onPress: () => void;
}) {
  return (
    <Pressable accessibilityRole="button" accessibilityLabel={label} onPress={onPress}>
      <ActionFace
        progress={progress}
        from={from}
        icon={icon}
        color={color}
        background={background}
      />
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  row: {
    flexDirection: "row",
    gap: 12,
    paddingVertical: 13,
    paddingLeft: 22,
    paddingRight: 14,
    borderRadius: radii.row,
    borderCurve: "continuous",
    backgroundColor: colors.background,
  },
  pressed: {
    backgroundColor: withAlpha(colors.content, 0.05),
  },
  dot: {
    position: "absolute",
    left: 7,
    top: "50%",
    width: 10,
    height: 10,
    marginTop: -5,
    borderRadius: 5,
    backgroundColor: colors.accent,
  },
  dotRead: {
    opacity: 0,
    transform: [{ scale: 0 }],
  },
  body: {
    flex: 1,
    minWidth: 0,
    gap: 3,
  },
  line: {
    flexDirection: "row",
    alignItems: "baseline",
    gap: 8,
  },
  title: {
    flex: 1,
    color: colors.content,
    fontSize: 16,
    fontWeight: "600",
  },
  titleRead: {
    fontWeight: "500",
  },
  time: {
    color: colors.faint,
    fontSize: 13,
    fontVariant: ["tabular-nums"],
  },
  timeUnread: {
    color: colors.accent,
  },
  why: {
    fontSize: 14,
    fontWeight: "500",
  },
  ref: {
    flex: 1,
    color: colors.faint,
    fontSize: 13,
  },
  stats: {
    flexDirection: "row",
    alignItems: "center",
    gap: 9,
  },
  stat: {
    color: colors.faint,
    fontSize: 13,
    fontVariant: ["tabular-nums"],
  },
  count: {
    flexDirection: "row",
    alignItems: "center",
    gap: 3,
  },
  actions: {
    flexDirection: "row",
    alignItems: "center",
    gap: ACTION_GAP,
    paddingLeft: ACTION_GAP,
    paddingRight: 4,
  },
  action: {
    width: ACTION_SIZE,
    height: ACTION_SIZE,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: radii.row,
    borderCurve: "continuous",
  },
}));
