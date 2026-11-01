import { useRef } from "react";
import { Pressable, Text, View } from "react-native";
import ReanimatedSwipeable, {
  type SwipeableMethods,
} from "react-native-gesture-handler/ReanimatedSwipeable";
import Animated, {
  interpolate,
  type EntryOrExitLayoutType,
  useAnimatedStyle,
  type SharedValue,
} from "react-native-reanimated";

import { AgentDots } from "@/features/orchestration/AgentDots";
import { relativeTime } from "@/features/workspace/relativeTime";
import type { SessionSummary } from "@/features/workspace/types";
import { Icon, type IconName } from "@/ui/Icon";
import { radii, withAlpha, themedStyles, useColors } from "@/ui/theme";

import { ProviderLogo } from "./ProviderLogo";
import { TerminalSpinner } from "./TerminalSpinner";

/** The swipe actions' buttons and the gap around them. */
const ACTION_SIZE = 40;
const ACTION_GAP = 8;

/**
 * A session as one line, like a chat in the Claude app's drawer: the
 * provider's mark where that app shows its chat icon, then the title. While
 * it works, the terminal spinner takes the mark's place, as in the desktop
 * sidebar. Laid out like the drawer's nav rows. Swiping it left uncovers
 * rename and delete, as in Mail; a long press offers the same.
 *
 * `detailed` lays it out as the desktop's SessionCard instead, for the
 * Sessions screen: see SessionDetails.
 */
export function SessionCard({
  session,
  active = false,
  onPress,
  onLongPress,
  onRename,
  onDelete,
  onSwipeOpen,
  entering,
  exiting,
  detailed = false,
}: {
  session: SessionSummary;
  active?: boolean;
  onPress: () => void;
  onLongPress?: () => void;
  onRename?: () => void;
  onDelete?: () => void;
  /** Told when this row's actions open, to close another row's. */
  onSwipeOpen?: (row: SwipeableMethods) => void;
  /** How the row comes into the list, when it is new. */
  entering?: EntryOrExitLayoutType;
  /** How the row leaves the list, when it is deleted. */
  exiting?: EntryOrExitLayoutType;
  /** Two lines with the time, branch and model, as on the desktop. */
  detailed?: boolean;
}) {
  const colors = useColors();
  const styles = useStyles();
  const swipeable = useRef<SwipeableMethods>(null);
  const opened = useRef(false);

  const row = (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ selected: active, busy: !!session.busy }}
      accessibilityLabel={session.busy ? `${session.title}, working` : session.title}
      // A tap on a row with its actions out puts them away instead.
      onPress={() => (opened.current ? swipeable.current?.close() : onPress())}
      onLongPress={onLongPress}
      accessibilityActions={onLongPress ? [{ name: "longpress", label: "Rename or delete" }] : undefined}
      onAccessibilityAction={({ nativeEvent }) => {
        if (nativeEvent.actionName === "longpress") onLongPress?.();
      }}
      style={({ pressed }) => [
        detailed ? styles.card : styles.row,
        active && styles.active,
        pressed && !active && styles.pressed,
      ]}
    >
      {detailed ? (
        <SessionDetails session={session} />
      ) : (
        <>
          <View style={styles.mark}>
            {session.busy ? (
              <TerminalSpinner style={styles.working} />
            ) : (
              <ProviderLogo harness={session.harness} size={20} color={colors.content} />
            )}
          </View>
          <Text style={styles.title} numberOfLines={1}>
            {session.title}
          </Text>
          {session.orchestration ? <AgentDots run={session.orchestration} /> : null}
        </>
      )}
    </Pressable>
  );
  if (!onRename || !onDelete) return row;

  // Each action puts the row back first, so it is closed behind the prompt.
  const act = (action: () => void) => () => {
    swipeable.current?.close();
    action();
  };
  return (
    <Animated.View entering={entering} exiting={exiting}>
      <ReanimatedSwipeable
        ref={swipeable}
        friction={1}
        rightThreshold={ACTION_SIZE}
        overshootRight={false}
        dragOffsetFromRightEdge={10}
        // Its actions are on the right, so only a swipe left is its own; one
        // right goes to whatever is around it, such as the page before.
        dragOffsetFromLeftEdge={Number.MAX_SAFE_INTEGER}
        onSwipeableWillOpen={() => {
          opened.current = true;
          if (swipeable.current) onSwipeOpen?.(swipeable.current);
        }}
        onSwipeableWillClose={() => {
          opened.current = false;
        }}
        renderRightActions={(progress) => (
          <RowActions progress={progress} onRename={act(onRename)} onDelete={act(onDelete)} />
        )}
      >
        {row}
      </ReanimatedSwipeable>
    </Animated.View>
  );
}

/**
 * A session as the desktop's Mono details panel lays out its SessionCard
 * (`modelInFooter`): the title with the time or status on its right, then the
 * branch with the model and its provider's mark on its right. The desktop
 * reveals the model's name on hover; with no hover here it always shows.
 */
function SessionDetails({ session }: { session: SessionSummary }) {
  const colors = useColors();
  const styles = useStyles();
  const model = session.modelName || session.model;
  return (
    <>
      <View style={styles.line}>
        <Text style={styles.cardTitle} numberOfLines={1}>
          {session.title}
        </Text>
        {session.orchestration ? <AgentDots run={session.orchestration} /> : null}
        {session.busy ? (
          <View style={styles.status}>
            <TerminalSpinner style={styles.statusLabel} />
            <Text style={styles.statusLabel}>Working...</Text>
          </View>
        ) : (
          <Text style={styles.time}>{relativeTime(session.updatedAt)}</Text>
        )}
      </View>
      <View style={styles.line}>
        <View style={styles.branch}>
          <Icon name="branch" size={13} color={colors.faint} />
          <Text style={[styles.meta, !session.branch && styles.noBranch]} numberOfLines={1}>
            {session.branch || "No branch"}
          </Text>
        </View>
        <View style={styles.model}>
          {model ? (
            <Text style={styles.meta} numberOfLines={1}>
              {model}
            </Text>
          ) : null}
          <ProviderLogo harness={session.harness} size={14} color={colors.muted} />
        </View>
      </View>
    </>
  );
}

/**
 * Rename and delete, uncovered from the right edge: delete, nearest the edge,
 * fades and grows in first, then rename, as the row slides aside.
 */
function RowActions({
  progress,
  onRename,
  onDelete,
}: {
  progress: SharedValue<number>;
  onRename: () => void;
  onDelete: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  return (
    <View style={styles.actions}>
      <RowAction
        progress={progress}
        from={0.35}
        icon="edit"
        label="Rename"
        color={colors.content}
        background={colors.surfaceRaised}
        onPress={onRename}
      />
      <RowAction
        progress={progress}
        from={0}
        icon="trash"
        label="Delete"
        color={colors.deletions}
        background={withAlpha(colors.deletions, 0.16)}
        onPress={onDelete}
      />
    </View>
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
  /** How far open the row is when this button starts to appear. */
  from: number;
  icon: IconName;
  label: string;
  color: string;
  background: string;
  onPress: () => void;
}) {
  const styles = useStyles();
  const style = useAnimatedStyle(() => {
    const shown = interpolate(progress.get(), [from, from + 0.65], [0, 1], "clamp");
    return { opacity: shown, transform: [{ scale: 0.7 + 0.3 * shown }] };
  });
  return (
    <Animated.View style={style}>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={label}
        onPress={onPress}
        style={({ pressed }) => [
          styles.action,
          { backgroundColor: background },
          pressed && styles.actionPressed,
        ]}
      >
        <Icon name={icon} size={18} color={color} />
      </Pressable>
    </Animated.View>
  );
}

const useStyles = themedStyles((colors) => ({
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    minHeight: 44,
    paddingHorizontal: 14,
    borderRadius: radii.row,
  },
  // As wide as the nav rows' icons, so titles start level with their labels.
  mark: {
    width: 22,
    alignItems: "center",
  },
  active: {
    backgroundColor: colors.selection,
  },
  pressed: {
    backgroundColor: withAlpha(colors.content, 0.05),
  },
  title: {
    flex: 1,
    color: colors.content,
    fontSize: 17,
  },
  // About the logo's size, on a fixed line so the frames don't shift the row.
  working: {
    color: colors.accent,
    fontSize: 18,
    lineHeight: 22,
    textAlign: "center",
  },
  card: {
    gap: 6,
    paddingHorizontal: 14,
    paddingVertical: 14,
    borderRadius: radii.row,
    borderCurve: "continuous",
  },
  line: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  cardTitle: {
    flex: 1,
    color: colors.content,
    fontSize: 16,
    fontWeight: "600",
  },
  time: {
    color: colors.faint,
    fontSize: 13,
    fontVariant: ["tabular-nums"],
  },
  status: {
    flexDirection: "row",
    alignItems: "center",
    gap: 4,
  },
  statusLabel: {
    color: colors.accent,
    fontSize: 13,
  },
  branch: {
    flex: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 5,
  },
  model: {
    flexShrink: 1,
    maxWidth: "50%",
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  meta: {
    flexShrink: 1,
    color: colors.muted,
    fontSize: 13,
  },
  noBranch: {
    color: colors.faint,
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
  },
  actionPressed: {
    opacity: 0.6,
  },
}));
