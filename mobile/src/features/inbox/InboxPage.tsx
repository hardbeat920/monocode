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
import { HEADER_HEIGHT, useSessionLayout } from "@/features/sessions/SessionScreen";
import { projectColor } from "@/features/workspace/projectColors";
import { relativeTime } from "@/features/workspace/relativeTime";
import { GlassSegments } from "@/ui/GlassSegments";
import { GradualBlur } from "@/ui/GradualBlur";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors, withAlpha } from "@/ui/theme";

import { Chip, FilterChips, FilterSheet, placeName, type FilterKind } from "./InboxFilters";
import { useInbox, useInboxActionsSupported, useInboxFeed } from "./InboxProvider";
import { GroupMenu } from "./GroupMenu";
import { InboxRow, type RowHandlers } from "./InboxRow";
import { SOURCE_NAMES } from "./ProviderMark";
import { InboxToast } from "./InboxToast";
import {
  bannerFor,
  groupItems,
  hasFacets,
  INBOX_CHIPS,
  optionsFrom,
  visibleItems,
  type InboxChip,
  type InboxFacets,
  type InboxGrouping,
} from "./model";
import type { InboxItem, InboxScope, InboxSource } from "./protocol";
import type { Feed } from "./store";
import { useNow } from "./useNow";

const SCOPES: { id: InboxScope; label: string }[] = [
  { id: "all", label: "All" },
  { id: "needsMe", label: "Needs me" },
  { id: "done", label: "Done" },
];

type Entry =
  | { type: "header"; id: string; title: string; count: number; projectId?: string }
  | { type: "row"; id: string; item: InboxItem };

/** The gaps around the scope switch, under the header and over the list. */
const SCOPE_ABOVE = 4;
const SCOPE_BELOW = 12;
/** What the scope switch takes below the header: the list starts this far down. */
export const SCOPE_SPACE = SCOPE_ABOVE + HEADER_HEIGHT + SCOPE_BELOW;

/**
 * The Inbox: review requests, mentions, assignments and failing checks from
 * the connected sources, for every project. The scope switch sits under the
 * header and the list scrolls under both. The chips open the desktop's filters
 * in partial sheets; `project`, the one it was opened from, heads the picker.
 */
export function InboxPage({ project }: { project?: { id: string; name: string } }) {
  const colors = useColors();
  const styles = useStyles();
  const reduced = useReducedMotion();
  const { mark, readAll, client } = useInbox();
  const { pageTop, dockBottom } = useSessionLayout();
  const bottom = dockBottom + SEARCH_BAR + 16;

  const [scope, setScope] = useState<InboxScope>("all");
  const [chips, setChips] = useState<InboxChip[]>([]);
  const [facets, setFacets] = useState<InboxFacets>({});
  const [sheet, setSheet] = useState<FilterKind | null>(null);
  const [grouping, setGrouping] = useState<InboxGrouping>("day");
  const [search, setSearch] = useState("");
  const wide = !facets.places?.length;

  const { feed: live, refresh, loadMore } = useInboxFeed({ scope, facets });
  const feed = useHeldFeed(live, scope);
  const canReadAll = useInboxActionsSupported();
  const now = useNow();
  const [pulling, setPulling] = useState(false);

  const shown = useMemo(
    () =>
      visibleItems(feed.items, { scope, chips, facets, query: search }, now),
    [feed.items, scope, chips, facets, search, now],
  );
  const options = useMemo(() => feed.options ?? optionsFrom(feed.items), [feed.options, feed.items]);
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

  const { places, ...others } = facets;
  const filtered = chips.length > 0 || hasFacets(others) || !!search.trim();
  const onePlace = places?.length === 1 ? places[0] : undefined;
  const banner = bannerFor(live);
  const exiting = reduced ? undefined : FadeOut.duration(140).reduceMotion(ReduceMotion.System);

  const header = (
    <View style={styles.header}>
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        keyboardShouldPersistTaps="handled"
        style={styles.chipScroll}
        contentContainerStyle={styles.chips}
      >
        <FilterChips
          facets={facets}
          onChange={setFacets}
          options={options}
          // An older desktop can't say who is assigned.
          canAssign={!!feed.options}
          names={names}
          client={client}
          onOpen={setSheet}
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
      </ScrollView>
      {banner ? <Banner banner={banner} now={now} onRetry={() => void refresh()} /> : null}
    </View>
  );

  return (
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
              <InboxRow
                item={entry.item}
                handlers={handlers}
                now={now}
                exiting={exiting}
                scoped={!wide}
              />
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
                projectName={onePlace ? placeName(onePlace, options, names) : undefined}
                onClear={() => {
                  setChips([]);
                  setFacets({});
                  setSearch("");
                }}
                onWiden={() => setFacets(others)}
              />
            ) : (
              <Skeleton />
            )
          }
          ListFooterComponent={
            live.loadingMore ? <ActivityIndicator style={styles.more} color={colors.muted} /> : null
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
          scrollIndicatorInsets={{ top: pageTop, bottom }}
          contentContainerStyle={[styles.list, { paddingTop: pageTop, paddingBottom: bottom }]}
          initialNumToRender={12}
          windowSize={7}
          keyboardDismissMode="interactive"
          keyboardShouldPersistTaps="handled"
          showsVerticalScrollIndicator={false}
        />
        <GradualBlur edge="top" height={pageTop} color={colors.background} />
        <GradualBlur edge="bottom" height={bottom} color={colors.background} />
        <View style={[styles.scopeBar, { top: pageTop - SCOPE_SPACE + SCOPE_ABOVE }]}>
          <ScopeSwitch
            value={scope}
            onChange={(next) => {
              closeOpenRow();
              setScope(next);
            }}
            needsMe={feed.counts.needsMe}
          />
        </View>
        <InboxToast bottom={bottom + 8} />
        <FileSearchBar
          value={search}
          onChange={setSearch}
          placeholder="Search inbox"
          accessory={
            <GroupMenu
              value={grouping}
              onChange={setGrouping}
              onMarkAllRead={
                canReadAll && feed.counts.unread > 0
                  ? () => void readAll({ scope, facets })
                  : undefined
              }
            />
          }
        />
        <FilterSheet
          open={sheet}
          onClose={() => setSheet(null)}
          facets={facets}
          onChange={setFacets}
          options={options}
          names={names}
          pinned={project?.id}
          client={client}
        />
    </View>
  );
}

/**
 * The list to show. A new filter is a new list that hasn't been read yet; the
 * last one stays up meanwhile, with the filter applied to it on the phone, so
 * a change shows at once instead of as a skeleton.
 */
function useHeldFeed(live: Feed, scope: InboxScope) {
  const [held, setHeld] = useState({ feed: live, scope });
  if (live.loaded && held.feed !== live) setHeld({ feed: live, scope });
  return live.loaded || held.scope !== scope || !held.feed.loaded ? live : held.feed;
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
  projectName?: string;
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
  scopeBar: {
    position: "absolute",
    left: 14,
    right: 14,
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
