import { useRef } from "react";
import { Pressable, Text, View } from "react-native";
import ReanimatedSwipeable, {
  type SwipeableMethods,
} from "react-native-gesture-handler/ReanimatedSwipeable";
import Animated, { type EntryOrExitLayoutType, type SharedValue } from "react-native-reanimated";

import { PixelMascot } from "@/features/mascots/PixelMascot";
import { relativeTime } from "@/features/workspace/relativeTime";
import type { SessionSummary } from "@/features/workspace/types";
import { Icon } from "@/ui/Icon";
import { ACTION_SIZE, SwipeAction, SwipeActions } from "@/ui/SwipeAction";
import { radii, withAlpha, themedStyles, useColors } from "@/ui/theme";

import type { SessionOrigin } from "./sessionMonos";
import { ProviderLogo } from "./ProviderLogo";
import { TerminalSpinner } from "./TerminalSpinner";

/**
 * A session as one line, like a chat in the Claude app's drawer: the
 * provider's mark where that app shows its chat icon, then the title. While
 * it works, the terminal spinner takes the mark's place, as in the desktop
 * sidebar. Laid out like the drawer's nav rows. Swiping it left uncovers
 * rename, delete and, where offered, archive, as in Mail; a long press
 * offers the same.
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
  onArchive,
  origin,
  onOpenOrigin,
  onOriginLongPress,
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
  /** Archives it; the swipe offers the button only with this. */
  onArchive?: () => void;
  /** Who started it, when a Mono did: a chip under the title. */
  origin?: SessionOrigin | null;
  /** The chip was tapped; the Mono's chat opens. */
  onOpenOrigin?: () => void;
  /** The chip was held, for its Mono's menu. */
  onOriginLongPress?: () => void;
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
      accessibilityActions={[
        ...(onLongPress ? [{ name: "longpress", label: "Rename or delete" }] : []),
        ...(onArchive ? [{ name: "archive", label: "Archive" }] : []),
      ]}
      onAccessibilityAction={({ nativeEvent }) => {
        if (nativeEvent.actionName === "longpress") onLongPress?.();
        if (nativeEvent.actionName === "archive") onArchive?.();
      }}
      style={({ pressed }) => [
        detailed ? styles.card : styles.row,
        active && styles.active,
        pressed && !active && styles.pressed,
      ]}
    >
      {detailed ? (
        <SessionDetails
          session={session}
          origin={origin}
          onOpenOrigin={onOpenOrigin}
          onOriginLongPress={onOriginLongPress}
        />
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
          <RowActions
            progress={progress}
            onRename={act(onRename)}
            onDelete={act(onDelete)}
            onArchive={onArchive ? act(onArchive) : undefined}
          />
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
function SessionDetails({
  session,
  origin,
  onOpenOrigin,
  onOriginLongPress,
}: {
  session: SessionSummary;
  origin?: SessionOrigin | null;
  onOpenOrigin?: () => void;
  onOriginLongPress?: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const model = session.modelName || session.model;
  return (
    <>
      <View style={styles.line}>
        <Text style={styles.cardTitle} numberOfLines={1}>
          {session.title}
        </Text>
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
      {origin ? (
        <OriginChip origin={origin} onPress={onOpenOrigin} onLongPress={onOriginLongPress} />
      ) : null}
    </>
  );
}

/**
 * Who started a session, on a line of its own under the meta so it never
 * crowds the branch and model: the Mono's mascot in its colour and "via" its
 * name, quiet. Tapping opens the Mono's chat. A Mono the desktop has since
 * deleted shows greyed and says so, with no tap.
 */
export function OriginChip({
  origin,
  onPress,
  onLongPress,
}: {
  origin: SessionOrigin;
  onPress?: () => void;
  onLongPress?: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const removed = origin.kind === "removed";
  const name = removed ? "Mono removed" : `via ${origin.mono.name}`;
  const tappable = !removed && origin.openable && !!onPress;
  // A removed Mono has no menu either.
  const held = removed ? undefined : onLongPress;
  return (
    <Pressable
      accessibilityRole={tappable ? "button" : "text"}
      accessibilityLabel={removed ? "Started by a Mono that was removed" : `Started by ${origin.mono.name}`}
      accessibilityHint={tappable ? "Opens the Mono's chat" : undefined}
      disabled={!tappable && !held}
      onPress={tappable ? onPress : undefined}
      onLongPress={held}
      hitSlop={{ top: 6, bottom: 6, left: 4, right: 12 }}
      style={({ pressed }) => [styles.chip, pressed && tappable && styles.chipPressed]}
    >
      <View style={styles.chipMascot}>
        {removed ? (
          <Icon name="bot" size={14} color={colors.faint} />
        ) : (
          <PixelMascot name={origin.mono.mascot} color={origin.mono.color} size={14} />
        )}
      </View>
      <Text style={[styles.chipLabel, removed && styles.chipRemoved]} numberOfLines={1}>
        {name}
      </Text>
    </Pressable>
  );
}

/**
 * Archive, delete and rename, uncovered from the right edge: archive, nearest
 * the edge, fades and grows in first, then delete, then rename, as the row
 * slides aside.
 */
function RowActions({
  progress,
  onRename,
  onDelete,
  onArchive,
}: {
  progress: SharedValue<number>;
  onRename: () => void;
  onDelete: () => void;
  onArchive?: () => void;
}) {
  const colors = useColors();
  return (
    <SwipeActions>
      <SwipeAction
        progress={progress}
        from={onArchive ? 0.4 : 0.35}
        icon="edit"
        label="Rename"
        color={colors.content}
        background={colors.surfaceRaised}
        onPress={onRename}
      />
      <SwipeAction
        progress={progress}
        from={onArchive ? 0.2 : 0}
        icon="trash"
        label="Delete"
        color={colors.deletions}
        background={withAlpha(colors.deletions, 0.16)}
        onPress={onDelete}
      />
      {onArchive ? (
        <SwipeAction
          progress={progress}
          from={0}
          icon="archive"
          label="Archive"
          color={colors.accent}
          background={withAlpha(colors.accent, 0.16)}
          onPress={onArchive}
        />
      ) : null}
    </SwipeActions>
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
  chip: {
    alignSelf: "flex-start",
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    maxWidth: "100%",
  },
  chipPressed: {
    opacity: 0.6,
  },
  chipMascot: {
    width: 14,
    height: 14,
  },
  chipLabel: {
    flexShrink: 1,
    color: colors.faint,
    fontSize: 13,
  },
  chipRemoved: {
    fontStyle: "italic",
  },
}));
