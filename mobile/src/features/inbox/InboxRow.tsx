import * as Haptics from "expo-haptics";
import { memo, useRef, useState } from "react";
import { Pressable, Text, View } from "react-native";
import ReanimatedSwipeable, {
  type SwipeableMethods,
} from "react-native-gesture-handler/ReanimatedSwipeable";
import Animated, {
  useAnimatedReaction,
  useSharedValue,
  type EntryOrExitLayoutType,
  type SharedValue,
} from "react-native-reanimated";
import { scheduleOnRN } from "react-native-worklets";

import { relativeTime } from "@/features/workspace/relativeTime";
import { Icon } from "@/ui/Icon";
import { ChoiceMenu } from "@/ui/ChoiceMenu";
import { ACTION_GAP, ACTION_SIZE, ActionFace, SwipeAction, SwipeActions } from "@/ui/SwipeAction";
import { useTextScale } from "@/ui/textScale";
import { radii, themedStyles, useColors, withAlpha } from "@/ui/theme";

import { ItemMark } from "./ItemMark";
import { snoozeOptions } from "./model";
import type { InboxItem } from "./protocol";

/** The buttons and the gaps around them: how far the row slides to show them. */
const actionsWidth = (buttons: number) => ACTION_SIZE * buttons + ACTION_GAP * buttons + 4;
const ACTIONS_WIDTH = actionsWidth(3);
/** How much farther past the buttons a swipe commits Done. */
const COMMIT_PAST = 64;
/** Above the row's top edge to its mark, and the mark's size at each fit. */
const ROW_PADDING = 13;
const MARK = 30;
const MARK_STACKED = 44;
const DOT = 10;
/** The largest a stat's icon gets, so it stays a detail beside its number. */
const STAT_ICON_MAX = 2;

/**
 * How a row arranges itself for the system text size. `roomy` gives the
 * reference and stats a line each, from XXL; `stacked` moves the mark and age
 * above the title so it gets the row's full width, from AX1.
 */
export type RowFit = "regular" | "roomy" | "stacked";

export function rowFit(textScale: number): RowFit {
  if (textScale >= 1.7) return "stacked";
  return textScale >= 1.2 ? "roomy" : "regular";
}

const LINES: Record<RowFit, { title: number; meta: number }> = {
  regular: { title: 2, meta: 1 },
  roomy: { title: 3, meta: 2 },
  stacked: { title: 4, meta: 3 },
};

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

/** What a row calls an item: GitHub's say which kind, as both number alike. */
export function itemRef(item: InboxItem) {
  if (item.source !== "github") return item.number;
  return [item.kind === "pr" ? "PR" : "Issue", item.number].filter(Boolean).join(" ");
}

/** Why a row is here, left out where its mark and reference already say it. */
function rowReason(item: InboxItem, now: number) {
  if (item.snoozedUntil && item.snoozedUntil > now) {
    return `Snoozed until ${snoozedLabel(item.snoozedUntil, now)}`;
  }
  const plain = item.source === "github" && !item.attentionLabel && item.status === "open";
  return plain ? undefined : whyLabel(item);
}

/**
 * An inbox item: what it is, then the title with its age, and under that its
 * reference, why it's here and where it lives, with its size, comments and
 * checks. Larger text rearranges it, as `rowFit` says. Swiping it left uncovers
 * Snooze, Start agent and Done, as in Mail; swiped all the way, it commits
 * Done. Right swipes are left to the page.
 */
export const InboxRow = memo(function InboxRow({
  item,
  handlers,
  now,
  exiting,
  scoped = false,
}: {
  item: InboxItem;
  handlers: RowHandlers;
  now: number;
  exiting?: EntryOrExitLayoutType;
  /** In a list of one project's items, which needn't each name it. */
  scoped?: boolean;
}) {
  const colors = useColors();
  const styles = useStyles();
  const swipeable = useRef<SwipeableMethods>(null);
  const opened = useRef(false);
  const armed = useSharedValue(false);
  // The buttons cost more to mount than the row they hide behind, and few rows
  // are ever swiped: until one is, an empty space holds their width.
  const [swiped, setSwiped] = useState(false);
  const textScale = useTextScale();
  const fit = rowFit(textScale);
  const markSize = fit === "stacked" ? MARK_STACKED : MARK;
  const statIcon = Math.round(14 * Math.min(Math.max(textScale, 1), STAT_ICON_MAX));

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

  const ref = itemRef(item);
  const reason = rowReason(item, now);
  const summary = [item.unread ? "Unread" : null, item.title, ref, reason, item.repo]
    .filter(Boolean)
    .join(", ");

  const title = (
    <Text
      numberOfLines={LINES[fit].title}
      style={[styles.title, !item.unread && styles.titleRead]}
    >
      {item.title}
    </Text>
  );
  const time = (
    <Text style={[styles.time, item.unread && styles.timeUnread]}>
      {relativeTime(item.updatedAt, now)}
    </Text>
  );
  const meta = (
    <Text numberOfLines={LINES[fit].meta} style={[styles.ref, fit === "regular" && styles.fill]}>
      {ref}
      {reason ? (
        <Text style={[styles.why, { color: tone }]}>
          {ref ? " · " : ""}
          {reason}
        </Text>
      ) : null}
      {scoped ? "" : `${ref || reason ? " · " : ""}${item.repo}`}
    </Text>
  );
  const sized = item.kind === "pr" && item.additions !== undefined;
  const stats =
    sized || item.comments > 0 || item.checks ? (
      <View style={[styles.stats, fit !== "regular" && styles.statsWrap]}>
        {sized ? (
          <>
            <Text style={[styles.stat, { color: colors.additions }]}>+{item.additions}</Text>
            <Text style={[styles.stat, { color: colors.deletions }]}>−{item.deletions ?? 0}</Text>
          </>
        ) : null}
        {item.comments > 0 ? (
          <View style={styles.count}>
            <Icon name="comment" size={statIcon} color={colors.faint} />
            <Text style={styles.stat}>{item.comments}</Text>
          </View>
        ) : null}
        {item.checks ? <ChecksGlyph state={item.checks} size={statIcon} /> : null}
      </View>
    ) : null;

  return (
    <Animated.View exiting={exiting}>
      <ReanimatedSwipeable
        ref={swipeable}
        friction={1}
        rightThreshold={ACTION_SIZE}
        overshootRight
        overshootFriction={4}
        dragOffsetFromRightEdge={10}
        // Its actions are on the right, so only a swipe left is its own.
        dragOffsetFromLeftEdge={Number.MAX_SAFE_INTEGER}
        onSwipeableOpenStartDrag={() => {
          armed.set(false);
          setSwiped(true);
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
        renderRightActions={(progress, translation) =>
          swiped ? (
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
          ) : (
            <View style={{ width: actionsWidth(item.done ? 2 : 3) }} />
          )
        }
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
          style={({ pressed }) => [
            styles.row,
            fit === "stacked" && styles.rowStacked,
            pressed && styles.pressed,
          ]}
        >
          <View
            pointerEvents="none"
            style={[
              styles.dot,
              // A tall row's middle is far from its mark: sit beside the mark.
              fit !== "regular" && { top: ROW_PADDING + (markSize - DOT) / 2, marginTop: 0 },
              !item.unread && styles.dotRead,
            ]}
          />
          {fit === "stacked" ? (
            <View style={styles.head}>
              <ItemMark item={item} size={markSize} />
              {time}
            </View>
          ) : (
            <ItemMark item={item} size={markSize} />
          )}
          <View style={[styles.body, fit === "stacked" && styles.bodyStacked]}>
            {fit === "stacked" ? (
              title
            ) : (
              <View style={styles.line}>
                {title}
                {time}
              </View>
            )}
            {fit === "regular" ? (
              <View style={styles.line}>
                {meta}
                {stats}
              </View>
            ) : (
              <>
                {meta}
                {stats}
              </>
            )}
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

export function ChecksGlyph({
  state,
  size = 14,
}: {
  state: NonNullable<InboxItem["checks"]>;
  size?: number;
}) {
  const colors = useColors();
  if (state === "passing") return <Icon name="passing" size={size} color={colors.additions} />;
  if (state === "failing") return <Icon name="failing" size={size} color={colors.deletions} />;
  return <Icon name="clock" size={size} color={colors.caution} />;
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
    <SwipeActions>
      {item.done ? (
        <SwipeAction
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
      <SwipeAction
        progress={progress}
        from={0.25}
        icon="sparkle"
        label="Start agent"
        color={colors.accent}
        background={withAlpha(colors.accent, 0.18)}
        onPress={onStart}
      />
      {item.done ? null : (
        <SwipeAction
          progress={progress}
          from={0}
          icon="check"
          label="Done"
          color={colors.additions}
          background={withAlpha(colors.additions, 0.18)}
          onPress={onDone}
        />
      )}
    </SwipeActions>
  );
}

const useStyles = themedStyles((colors) => ({
  row: {
    flexDirection: "row",
    gap: 12,
    paddingVertical: ROW_PADDING,
    paddingLeft: 22,
    paddingRight: 14,
    borderRadius: radii.row,
    borderCurve: "continuous",
    backgroundColor: colors.background,
  },
  rowStacked: {
    flexDirection: "column",
    gap: 8,
  },
  pressed: {
    backgroundColor: withAlpha(colors.content, 0.05),
  },
  dot: {
    position: "absolute",
    left: 7,
    top: "50%",
    width: DOT,
    height: DOT,
    marginTop: -DOT / 2,
    borderRadius: DOT / 2,
    backgroundColor: colors.accent,
  },
  dotRead: {
    opacity: 0,
    transform: [{ scale: 0 }],
  },
  head: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 8,
  },
  body: {
    flex: 1,
    minWidth: 0,
    gap: 3,
  },
  // In a column, `flex: 1` would squeeze it to no height.
  bodyStacked: {
    flex: 0,
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
    fontWeight: "500",
  },
  ref: {
    color: colors.faint,
    fontSize: 13,
  },
  fill: {
    flex: 1,
  },
  stats: {
    flexDirection: "row",
    alignItems: "center",
    gap: 9,
  },
  statsWrap: {
    flexWrap: "wrap",
    rowGap: 2,
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
}));
