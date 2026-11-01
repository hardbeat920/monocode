import { memo, useEffect, useState } from "react";
import { Pressable, Text, View } from "react-native";
import Animated, {
  Easing,
  FadeIn,
  FadeOut,
  LinearTransition,
  ReduceMotion,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
} from "react-native-reanimated";

import { PixelMascot } from "@/features/mascots/PixelMascot";
import { monoDetail, when } from "@/features/workspace/monoRow";
import type { Mono } from "@/features/workspace/types";
import { Collapsible } from "@/ui/Collapsible";
import { Icon } from "@/ui/Icon";
import { radii, themedStyles, useColors, withAlpha } from "@/ui/theme";

import { groupSize, type MonoGroup as Group } from "./sessionMonos";

/**
 * The Monos pinned above a project's sessions: a header that folds the group,
 * with a plus to make a Mono, then a compact row for each, as a chat list
 * has them: mascot, name, last line, time and an unread dot. Monos with no
 * project sit under "Unassigned".
 */
export function MonoGroup({
  group,
  unread,
  collapsed,
  onToggle,
  onOpen,
  onNew,
}: {
  group: Group;
  /** The Monos with a reply the user hasn't seen. */
  unread: ReadonlySet<string>;
  collapsed: boolean;
  onToggle: () => void;
  onOpen: (mono: Mono) => void;
  /** Makes a Mono; absent where the desktop can't. */
  onNew?: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const count = groupSize(group);
  const anyUnread = [...group.assigned, ...group.unassigned].some((mono) => unread.has(mono.id));
  // Rows already here at the first render don't fade in; ones that arrive
  // after it do, and the rest slide to make room.
  const [settled, setSettled] = useState(false);
  useEffect(() => {
    const frame = requestAnimationFrame(() => setSettled(true));
    return () => cancelAnimationFrame(frame);
  }, []);
  const turn = useSharedValue(collapsed ? 1 : 0);
  useEffect(() => {
    turn.set(withTiming(collapsed ? 1 : 0, { duration: 280, easing: Easing.bezier(0.4, 0, 0.2, 1) }));
  }, [collapsed, turn]);
  const chevron = useAnimatedStyle(() => ({ transform: [{ rotate: `${-90 * turn.get()}deg` }] }));
  const row = (mono: Mono) => (
    <Animated.View
      key={mono.id}
      layout={LinearTransition.duration(220).reduceMotion(ReduceMotion.System)}
      entering={settled ? FadeIn.duration(220).reduceMotion(ReduceMotion.System) : undefined}
      exiting={FadeOut.duration(140).reduceMotion(ReduceMotion.System)}
    >
      <MonoRow mono={mono} unread={unread.has(mono.id)} onOpen={onOpen} />
    </Animated.View>
  );
  return (
    <View style={styles.group}>
      <View style={styles.header}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`Monos, ${count}`}
          accessibilityState={{ expanded: !collapsed }}
          onPress={onToggle}
          style={({ pressed }) => [styles.toggle, pressed && styles.pressed]}
        >
          <Animated.View style={chevron}>
            <Icon name="chevronDown" size={16} color={colors.muted} />
          </Animated.View>
          <Text style={styles.heading}>Monos</Text>
          <Text style={styles.count}>{count}</Text>
          {collapsed && anyUnread ? <View style={styles.dot} /> : null}
        </Pressable>
        {onNew ? (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="New Mono"
            hitSlop={8}
            onPress={onNew}
            style={({ pressed }) => [styles.plus, pressed && styles.pressed]}
          >
            <Icon name="add" size={18} color={colors.content} />
          </Pressable>
        ) : null}
      </View>
      <Collapsible open={!collapsed}>
        {group.assigned.map(row)}
        {group.unassigned.length ? (
          <>
            <Text style={styles.subheading}>Unassigned</Text>
            {group.unassigned.map(row)}
          </>
        ) : null}
      </Collapsible>
      <View style={styles.rule} />
    </View>
  );
}

const MonoRow = memo(function MonoRow({
  mono,
  unread,
  onOpen,
}: {
  mono: Mono;
  unread: boolean;
  onOpen: (mono: Mono) => void;
}) {
  const styles = useStyles();
  const detail = monoDetail(mono);
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`${unread ? "Unread, " : ""}${mono.name}, ${detail}`}
      onPress={() => onOpen(mono)}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
    >
      <PixelMascot name={mono.mascot} color={mono.color} status={mono.status} size={30} centered />
      <View style={styles.text}>
        <View style={styles.line}>
          <Text style={[styles.name, unread && styles.nameUnread]} numberOfLines={1}>
            {mono.name}
          </Text>
          {mono.lastActiveAt ? (
            <Text style={[styles.time, unread && styles.timeUnread]}>{when(mono.lastActiveAt)}</Text>
          ) : null}
        </View>
        <View style={styles.line}>
          <Text
            style={[styles.detail, mono.status === "needs-you" && styles.needsYou]}
            numberOfLines={1}
          >
            {detail}
          </Text>
          {unread ? <View style={styles.dot} /> : null}
        </View>
      </View>
    </Pressable>
  );
});

const useStyles = themedStyles((colors) => ({
  group: {
    paddingBottom: 4,
  },
  header: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    paddingLeft: 14,
    paddingRight: 8,
    minHeight: 40,
  },
  toggle: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    minHeight: 40,
    paddingRight: 12,
  },
  heading: {
    color: colors.muted,
    fontSize: 13,
    fontWeight: "600",
  },
  count: {
    color: colors.faint,
    fontSize: 13,
    fontVariant: ["tabular-nums"],
  },
  plus: {
    width: 32,
    height: 32,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 16,
    backgroundColor: withAlpha(colors.content, 0.06),
  },
  pressed: {
    opacity: 0.6,
  },
  subheading: {
    paddingHorizontal: 14,
    paddingTop: 8,
    paddingBottom: 2,
    color: colors.faint,
    fontSize: 12,
    fontWeight: "600",
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    minHeight: 52,
    paddingHorizontal: 14,
    paddingVertical: 8,
    borderRadius: radii.row,
    borderCurve: "continuous",
  },
  text: {
    flex: 1,
    minWidth: 0,
    gap: 2,
  },
  line: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  name: {
    flex: 1,
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
  },
  nameUnread: {
    fontWeight: "700",
  },
  time: {
    color: colors.faint,
    fontSize: 12,
    fontVariant: ["tabular-nums"],
  },
  timeUnread: {
    color: colors.accent,
  },
  detail: {
    flex: 1,
    color: colors.muted,
    fontSize: 13,
  },
  needsYou: {
    color: colors.caution,
  },
  dot: {
    width: 8,
    height: 8,
    borderRadius: 4,
    backgroundColor: colors.accent,
  },
  rule: {
    height: 1,
    marginHorizontal: 14,
    marginTop: 6,
    backgroundColor: colors.stroke,
  },
}));
