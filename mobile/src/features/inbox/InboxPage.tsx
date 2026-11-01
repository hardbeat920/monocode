import * as Haptics from "expo-haptics";
import { router } from "expo-router";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  RefreshControl,
  ScrollView,
  Text,
  View,
} from "react-native";
import type { SwipeableMethods } from "react-native-gesture-handler/ReanimatedSwipeable";
import Animated, {
  Easing,
  FadeOut,
  LinearTransition,
  ReduceMotion,
  useAnimatedStyle,
  useReducedMotion,
  useSharedValue,
  withRepeat,
  withSequence,
  withTiming,
  type SharedValue,
} from "react-native-reanimated";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { FileSearchBar, SEARCH_BAR } from "@/features/files/FileSearchBar";
import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { useSessionLayout } from "@/features/sessions/SessionScreen";
import { projectColor } from "@/features/workspace/projectColors";
import { relativeTime } from "@/features/workspace/relativeTime";
import { AnimationsPaused } from "@/ui/AnimationsPaused";
import { useKeepsSwipe } from "@/ui/ScenePager";
import { GlassSegments } from "@/ui/GlassSegments";
import { GradualBlur } from "@/ui/GradualBlur";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors, withAlpha } from "@/ui/theme";

import { useInbox, useInboxFeed } from "./InboxProvider";
import { GroupMenu } from "./GroupMenu";
import { InboxRow, type RowHandlers } from "./InboxRow";
import { SOURCE_NAMES, useSourceColor } from "./ProviderMark";
import { InboxToast } from "./InboxToast";
import {
  bannerFor,
  groupItems,
  INBOX_CHIPS,
  sourcesIn,
  visibleItems,
  type InboxChip,
  type InboxGrouping,
} from "./model";
import type { InboxItem, InboxScope, InboxSource } from "./protocol";
import { useNow } from "./useNow";

const SCOPES: { id: InboxScope; label: string }[] = [
  { id: "all", label: "All" },
  { id: "needsMe", label: "Needs me" },
  { id: "done", label: "Done" },
];

type Entry =
  | { type: "header"; id: string; title: string; count: number; projectId?: string }
  | { type: "row"; id: string; item: InboxItem };

/**
 * The project's Inbox, the third page after New and Sessions: review requests,
 * mentions, assignments and failing checks from the connected sources. It
 * starts on this project's items, with a chip to take in every project's.
 * `active` while the page is in view: it polls then.
 */
export function InboxPage({
  projectId,
  projectName,
  active,
}: {
  projectId: string;
  projectName: string;
  active: boolean;
}) {
  const colors = useColors();
  const styles = useStyles();
  const reduced = useReducedMotion();
  const keepsSwipe = useKeepsSwipe();
  const { mark } = useInbox();
  const { pageTop, dockBottom } = useSessionLayout();
  const bottom = dockBottom + SEARCH_BAR + 16;

  const [scope, setScope] = useState<InboxScope>("all");
  const [wide, setWide] = useState(false);
  const [chips, setChips] = useState<InboxChip[]>([]);
  const [sources, setSources] = useState<InboxSource[]>([]);
  const [grouping, setGrouping] = useState<InboxGrouping>("day");
  const [search, setSearch] = useState("");
  const scoped = wide ? undefined : projectId;

  const { feed, refresh, loadMore } = useInboxFeed({ scope, projectId: scoped }, active);
  const now = useNow(active);
  const [pulling, setPulling] = useState(false);

  const shown = useMemo(
    () =>
      visibleItems(
        feed.items,
        { scope, projectId: scoped, chips, sources, query: search },
        now,
      ),
    [feed.items, scope, scoped, chips, sources, search, now],
  );
  const available = useMemo(() => sourcesIn(feed.items), [feed.items]);
  const names = useProjectNames();
  const entries = useMemo<Entry[]>(
    () =>
      groupItems(shown, grouping, now, (id, repo) => (id ? (names.get(id) ?? repo) : repo)).flatMap(
        (group) => [
          {
            type: "header" as const,
            id: `header:${group.id}`,
            title: group.title,
            count: group.items.length,
            projectId: grouping === "project" ? group.items[0]?.projectId : undefined,
          },
          ...group.items.map((item) => ({ type: "row" as const, id: item.key, item })),
        ],
      ),
    [shown, grouping, now, names],
  );

  // One row's actions out at a time; scrolling or leaving puts them away.
  const openRow = useRef<SwipeableMethods | null>(null);
  const closeOpenRow = useCallback(() => {
    openRow.current?.close();
    openRow.current = null;
  }, []);
  useEffect(() => {
    if (!active) closeOpenRow();
  }, [active, closeOpenRow]);

  const latest = useRef({ mark });
  useLayoutEffect(() => {
    latest.current = { mark };
  });
  const handlers = useMemo<RowHandlers>(
    () => ({
      open: (item) =>
        router.push({ pathname: "/inbox/item", params: { key: item.key } }),
      done: (item) => void latest.current.mark(item, "done"),
      undone: (item) => void latest.current.mark(item, "undone"),
      snooze: (item, until) => void latest.current.mark(item, "snooze", until),
      startAgent: (item) =>
        router.push({ pathname: "/inbox/start", params: { key: item.key } }),
      swipeOpened: (row) => {
        if (openRow.current !== row) openRow.current?.close();
        openRow.current = row;
      },
    }),
    [],
  );

  const filtered = chips.length > 0 || sources.length > 0 || !!search.trim();
  const banner = bannerFor(feed);
  const exiting = reduced ? undefined : FadeOut.duration(140).reduceMotion(ReduceMotion.System);

  const header = (
    <View style={styles.header}>
      <ScopeSwitch
        value={scope}
        onChange={(next) => {
          closeOpenRow();
          setScope(next);
        }}
        needsMe={feed.counts.needsMe}
      />
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        keyboardShouldPersistTaps="handled"
        {...keepsSwipe}
        style={styles.chipScroll}
        contentContainerStyle={styles.chips}
      >
        <Chip
          label={wide ? "All projects" : projectName}
          selected={!wide}
          onPress={() => setWide(!wide)}
          leading={
            wide ? (
              <Icon name="folder" size={16} color={colors.muted} />
            ) : (
              <ProjectMascot project={projectId} color={projectColor(projectId)} size={16} />
            )
          }
          hint={wide ? "Show only this project" : "Show every project"}
        />
        {INBOX_CHIPS.map((chip) => (
          <Chip
            key={chip.id}
            label={chip.label}
            count={chip.id === "unread" && feed.counts.unread > 0 ? feed.counts.unread : undefined}
            selected={chips.includes(chip.id)}
            onPress={() =>
              setChips((current) =>
                current.includes(chip.id)
                  ? current.filter((entry) => entry !== chip.id)
                  : [...current, chip.id],
              )
            }
          />
        ))}
        {available.length > 1 || sources.length > 0
          ? available.map((source) => (
              <SourceChip
                key={source}
                source={source}
                selected={sources.includes(source)}
                onPress={() =>
                  setSources((current) =>
                    current.includes(source)
                      ? current.filter((entry) => entry !== source)
                      : [...current, source],
                  )
                }
              />
            ))
          : null}
      </ScrollView>
      {banner ? <Banner banner={banner} now={now} onRetry={() => void refresh()} /> : null}
    </View>
  );

  return (
    <AnimationsPaused value={!active}>
      <View style={styles.page}>
        <Animated.FlatList
          data={feed.loaded ? entries : []}
          keyExtractor={(entry) => entry.id}
          itemLayoutAnimation={
            reduced ? undefined : LinearTransition.duration(220).reduceMotion(ReduceMotion.System)
          }
          renderItem={({ item: entry }) =>
            entry.type === "header" ? (
              <GroupHeader entry={entry} />
            ) : (
              <InboxRow item={entry.item} handlers={handlers} now={now} exiting={exiting} />
            )
          }
          ListHeaderComponent={header}
          ListEmptyComponent={
            feed.loaded ? (
              <EmptyState
                scope={scope}
                filtered={filtered}
                wide={wide}
                connected={feed.providers.some((provider) => provider.connected)}
                offline={feed.offline}
                projectName={projectName}
                onClear={() => {
                  setChips([]);
                  setSources([]);
                  setSearch("");
                }}
                onWiden={() => setWide(true)}
              />
            ) : (
              <Skeleton />
            )
          }
          ListFooterComponent={
            feed.loadingMore ? <ActivityIndicator style={styles.more} color={colors.muted} /> : null
          }
          onScrollBeginDrag={closeOpenRow}
          onEndReached={() => void loadMore()}
          onEndReachedThreshold={0.5}
          refreshControl={
            <RefreshControl
              refreshing={pulling}
              tintColor={colors.muted}
              progressViewOffset={pageTop}
              onRefresh={() => {
                setPulling(true);
                void Haptics.selectionAsync();
                void refresh().finally(() => setPulling(false));
              }}
            />
          }
          contentInset={{ top: pageTop }}
          contentOffset={{ x: 0, y: -pageTop }}
          scrollIndicatorInsets={{ top: pageTop, bottom }}
          contentContainerStyle={[styles.list, { paddingBottom: bottom }]}
          initialNumToRender={12}
          windowSize={7}
          keyboardDismissMode="interactive"
          keyboardShouldPersistTaps="handled"
          showsVerticalScrollIndicator={false}
        />
        <GradualBlur edge="bottom" height={bottom} color={colors.background} />
        <InboxToast bottom={bottom + 8} />
        <FileSearchBar
          value={search}
          onChange={setSearch}
          placeholder="Search inbox"
          accessory={<GroupMenu value={grouping} onChange={setGrouping} />}
        />
      </View>
    </AnimationsPaused>
  );
}

/** Phone project ids to their names, for the project grouping. */
function useProjectNames() {
  const { projects } = useConnection().catalog;
  return useMemo(() => new Map(projects.map((project) => [project.id, project.name])), [projects]);
}


function GroupHeader({ entry }: { entry: Extract<Entry, { type: "header" }> }) {
  const colors = useColors();
  const styles = useStyles();
  return (
    <View style={styles.groupHeader} accessibilityRole="header">
      {entry.projectId ? (
        <ProjectMascot
          project={entry.projectId}
          color={projectColor(entry.projectId)}
          size={16}
        />
      ) : null}
      <Text style={styles.groupTitle}>{entry.title}</Text>
      <Text style={[styles.groupCount, { color: colors.faint }]}>{entry.count}</Text>
    </View>
  );
}

/** All / Needs me / Done, in the same glass as the page switch above. */
function ScopeSwitch({
  value,
  onChange,
  needsMe,
}: {
  value: InboxScope;
  onChange: (value: InboxScope) => void;
  needsMe: number;
}) {
  const styles = useStyles();
  return (
    <GlassSegments
      style={styles.scopes}
      value={value}
      onChange={onChange}
      segments={SCOPES.map((scope) => ({
        value: scope.id,
        label: scope.label,
        trailing:
          scope.id === "needsMe" && needsMe > 0 ? (
            <View style={styles.count}>
              <Text style={styles.countLabel}>{needsMe}</Text>
            </View>
          ) : undefined,
      }))}
    />
  );
}

function Chip({
  label,
  selected,
  onPress,
  leading,
  count,
  hint,
}: {
  label: string;
  selected: boolean;
  onPress: () => void;
  leading?: React.ReactNode;
  count?: number;
  hint?: string;
}) {
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityHint={hint}
      accessibilityState={{ selected }}
      onPress={() => {
        void Haptics.selectionAsync();
        onPress();
      }}
      style={[styles.chip, selected && styles.chipOn]}
    >
      {leading}
      <Text style={[styles.chipLabel, selected && styles.chipLabelOn]}>{label}</Text>
      {count ? <Text style={[styles.chipCount, selected && styles.chipLabelOn]}>{count}</Text> : null}
    </Pressable>
  );
}

function SourceChip({
  source,
  selected,
  onPress,
}: {
  source: InboxSource;
  selected: boolean;
  onPress: () => void;
}) {
  const tint = useSourceColor(source);
  return (
    <Chip
      label={SOURCE_NAMES[source]}
      selected={selected}
      onPress={onPress}
      leading={<View style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: tint }} />}
    />
  );
}

function Banner({
  banner,
  now,
  onRetry,
}: {
  banner: NonNullable<ReturnType<typeof bannerFor>>;
  now: number;
  onRetry: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const offline = banner.kind === "offline";
  const since = banner.fetchedAt ? `Showing the last sync, ${relativeTime(banner.fetchedAt, now)} ago.` : "";
  const text = offline
    ? `Can’t reach your desktop. ${since}`.trim()
    : banner.failing.length > 0
      ? `${banner.failing.map((id) => SOURCE_NAMES[id as InboxSource] ?? id).join(", ")} didn’t update. ${since}`
      : `This may be out of date. ${since}`;
  return (
    <View
      accessibilityRole="alert"
      style={[styles.banner, { backgroundColor: withAlpha(colors.caution, 0.14) }]}
    >
      <Icon name={offline ? "offline" : "alert"} size={18} color={colors.caution} />
      <Text style={styles.bannerText}>{text}</Text>
      {offline ? (
        <Pressable accessibilityRole="button" accessibilityLabel="Retry" onPress={onRetry}>
          <Text style={[styles.bannerAction, { color: colors.accent }]}>Retry</Text>
        </Pressable>
      ) : null}
    </View>
  );
}

function EmptyState({
  scope,
  filtered,
  wide,
  connected,
  offline,
  projectName,
  onClear,
  onWiden,
}: {
  scope: InboxScope;
  filtered: boolean;
  wide: boolean;
  connected: boolean;
  offline: boolean;
  projectName: string;
  onClear: () => void;
  onWiden: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  let title = "All caught up";
  let body = wide
    ? "New items from your connected sources land here."
    : `Nothing new for ${projectName}.`;
  let action: { label: string; run: () => void } | null = wide
    ? null
    : { label: "Show every project", run: onWiden };
  if (filtered) {
    title = "No matches";
    body = "Nothing fits those filters.";
    action = { label: "Clear filters", run: onClear };
  } else if (scope === "done") {
    title = "Nothing done yet";
    body = "Items you mark done wait here.";
  } else if (scope === "needsMe") {
    body = "No review requests, mentions or failing checks.";
  } else if (!connected && !offline) {
    title = "Nothing here yet";
    body =
      "Connect GitHub, GitLab, Jira, Linear or Azure DevOps on your desktop and their review requests, mentions and failing checks land here.";
    action = null;
  }
  return (
    <View style={styles.empty}>
      <ProjectMascot project="inbox" name="ghost" color={colors.faint} size={64} />
      <Text style={styles.emptyTitle}>{title}</Text>
      <Text style={styles.emptyBody}>{body}</Text>
      {action ? (
        <Pressable accessibilityRole="button" onPress={action.run} hitSlop={8}>
          <Text style={[styles.emptyAction, { color: colors.accent }]}>{action.label}</Text>
        </Pressable>
      ) : null}
    </View>
  );
}

/** Rows' worth of placeholders that breathe while the first read is out. */
function Skeleton() {
  const styles = useStyles();
  const reduced = useReducedMotion();
  const pulse = useSharedValue(1);
  useEffect(() => {
    if (reduced) return;
    pulse.set(
      withRepeat(
        withSequence(
          withTiming(0.5, { duration: 700, easing: Easing.inOut(Easing.quad) }),
          withTiming(1, { duration: 700, easing: Easing.inOut(Easing.quad) }),
        ),
        -1,
      ),
    );
  }, [pulse, reduced]);
  return (
    <View accessibilityLabel="Loading inbox" accessibilityRole="progressbar">
      {[0, 1, 2, 3, 4, 5].map((row) => (
        <View key={row} style={styles.skeletonRow}>
          <Block pulse={pulse} style={styles.skeletonMark} />
          <View style={styles.skeletonBody}>
            <Block pulse={pulse} style={{ height: 14, width: `${70 - (row % 3) * 12}%` }} />
            <Block pulse={pulse} style={{ height: 12, width: "52%" }} />
            <Block pulse={pulse} style={{ height: 11, width: "38%" }} />
          </View>
        </View>
      ))}
    </View>
  );
}

function Block({
  pulse,
  style,
}: {
  pulse: SharedValue<number>;
  style: object;
}) {
  const styles = useStyles();
  const animated = useAnimatedStyle(() => ({ opacity: pulse.get() }));
  return <Animated.View style={[styles.block, style, animated]} />;
}

const useStyles = themedStyles((colors) => ({
  page: {
    flex: 1,
  },
  list: {
    paddingHorizontal: 10,
  },
  header: {
    gap: 10,
    paddingBottom: 4,
  },
  scopes: {
    marginHorizontal: 4,
  },
  count: {
    minWidth: 18,
    height: 18,
    paddingHorizontal: 5,
    borderRadius: 9,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.accent,
  },
  countLabel: {
    color: colors.inverse,
    fontSize: 11,
    fontWeight: "700",
  },
  // Bleeds past the list's side padding so the chips scroll to the screen's
  // edges rather than clipping a few points short of them.
  chipScroll: {
    marginHorizontal: -10,
  },
  chips: {
    gap: 6,
    paddingHorizontal: 14,
  },
  chip: {
    height: 32,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingHorizontal: 12,
    borderRadius: 16,
    backgroundColor: colors.fill,
  },
  chipOn: {
    backgroundColor: withAlpha(colors.accent, 0.18),
  },
  chipLabel: {
    color: colors.muted,
    fontSize: 14,
    fontWeight: "500",
  },
  chipLabelOn: {
    color: colors.accent,
  },
  chipCount: {
    color: colors.muted,
    fontSize: 13,
    fontVariant: ["tabular-nums"],
  },
  banner: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    marginHorizontal: 4,
    paddingVertical: 10,
    paddingHorizontal: 14,
    borderRadius: 12,
    borderCurve: "continuous",
  },
  bannerText: {
    flex: 1,
    color: colors.content,
    fontSize: 14,
  },
  bannerAction: {
    fontSize: 14,
    fontWeight: "700",
  },
  groupHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    paddingTop: 16,
    paddingBottom: 6,
    paddingHorizontal: 14,
  },
  groupTitle: {
    color: colors.content,
    fontSize: 13,
    fontWeight: "500",
  },
  groupCount: {
    marginLeft: "auto",
    fontSize: 13,
  },
  more: {
    paddingVertical: 16,
  },
  empty: {
    alignItems: "center",
    gap: 6,
    paddingTop: 72,
    paddingHorizontal: 36,
  },
  emptyTitle: {
    marginTop: 14,
    color: colors.content,
    fontSize: 20,
    fontWeight: "600",
  },
  emptyBody: {
    maxWidth: 290,
    color: colors.muted,
    fontSize: 15,
    lineHeight: 21,
    textAlign: "center",
  },
  emptyAction: {
    marginTop: 10,
    fontSize: 15,
    fontWeight: "600",
  },
  skeletonRow: {
    flexDirection: "row",
    gap: 12,
    paddingVertical: 14,
    paddingLeft: 22,
    paddingRight: 14,
  },
  skeletonMark: {
    width: 30,
    height: 30,
    borderRadius: 9,
  },
  skeletonBody: {
    flex: 1,
    gap: 9,
    paddingTop: 2,
  },
  block: {
    borderRadius: 6,
    backgroundColor: colors.fill,
  },
}));
