import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { ActivityIndicator, StyleSheet, Text, View } from "react-native";
import type { SwipeableMethods } from "react-native-gesture-handler/ReanimatedSwipeable";
import Animated, { FadeIn, FadeOut, LinearTransition } from "react-native-reanimated";

import { FileSearchBar, SEARCH_BAR } from "@/features/files/FileSearchBar";
import type { Harness, SessionSummary, Worktree } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { GradualBlur } from "@/ui/GradualBlur";
import { themedStyles, useColors } from "@/ui/theme";

import { SessionCard } from "./SessionCard";
import {
  filterSessions,
  NO_FILTER,
  SessionFilterMenu,
  type SessionFilter,
} from "./SessionFilterMenu";
import { useSessionLayout } from "./SessionScreen";
import { SpinnersPaused } from "./TerminalSpinner";
import { ARRIVE_MS, useArrivals } from "./useArrivals";
import { REMOVE_MS, useSessionActions } from "./useSessionActions";
import { useSessionPages, visibleSessions } from "./useSessionPages";

/**
 * A project's sessions, newest first and paged in as the list scrolls. Each
 * swipes left to rename or delete, as on the desktop sidebar. Searched and
 * filtered from the bar where the composer rests on the chat page, and
 * narrowed to `worktree` when the header's menu picks one, as the desktop's
 * sidebar is. `active` while the page is in view: it reads the list then, and
 * its spinners only run then.
 */
export function SessionsPage({
  projectId,
  active,
  worktree,
  onOpen,
}: {
  projectId: string;
  active: boolean;
  worktree?: Worktree;
  onOpen: (session: SessionSummary) => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const { sessions, edited, rememberSession } = useWorkspace();
  const { pageTop, dockBottom } = useSessionLayout();
  // Room for the search bar where the composer rests, and the blur under it.
  const bottom = dockBottom + SEARCH_BAR + 16;
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<SessionFilter>(NO_FILTER);
  const needle = query.trim().toLowerCase();
  const recent = sessions.filter((session) => session.projectId === projectId);
  const pages = useSessionPages(projectId, needle, active);
  const listed = visibleSessions(pages, recent, needle, edited).filter((session) =>
    inWorktree(session, worktree),
  );
  const visible = filterSessions(listed, filter);
  const scope = JSON.stringify([projectId, needle, filter, worktree?.path, !!pages]);
  const arrivals = useArrivals(visible, scope, active);
  // The agents to filter by: those in the list, in the order first seen.
  const harnesses = [...new Set<Harness>(listed.map((session) => session.harness))];
  if (filter.harness && !harnesses.includes(filter.harness)) harnesses.push(filter.harness);
  const narrowed = !!needle || filter !== NO_FILTER || !!worktree;

  const actions = useSessionActions(() => {});
  const { closeOpenRow } = actions;
  useEffect(() => {
    if (!active) closeOpenRow();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  // The rows' handlers keep their identity, reaching the latest render's
  // through a ref, so a row only re-renders when its own session changes.
  const latest = useRef({ onOpen, rememberSession, actions });
  useLayoutEffect(() => {
    latest.current = { onOpen, rememberSession, actions };
  });
  const handlers = useMemo<RowHandlers>(
    () => ({
      open: (session) => {
        latest.current.rememberSession(session);
        latest.current.onOpen(session);
      },
      showActions: (session) => latest.current.actions.showActions(session),
      rename: (session) => latest.current.actions.rename(session),
      remove: (session) => latest.current.actions.remove(session),
      swipeOpened: (row) => latest.current.actions.swipeOpened(row),
    }),
    [],
  );
  const removingId = actions.removing?.id;
  const { arriving, moving } = arrivals;
  const renderItem = useCallback(
    ({ item }: { item: SessionSummary }) => (
      <SessionRow
        session={item}
        removing={removingId === item.id}
        arriving={arriving.has(item.id)}
        handlers={handlers}
      />
    ),
    [removingId, arriving, handlers],
  );

  return (
    <SpinnersPaused value={!active}>
      <View style={styles.page}>
        <Animated.FlatList
          itemLayoutAnimation={
            actions.removing
              ? LinearTransition.duration(REMOVE_MS)
              : moving
                ? LinearTransition.duration(ARRIVE_MS)
                : undefined
          }
          data={arrivals.sessions}
          // Re-renders the rows when they start or stop moving, so each takes up the transition.
          extraData={moving}
          keyExtractor={(session) => session.id}
          renderItem={renderItem}
          ItemSeparatorComponent={Divider}
          ListEmptyComponent={
            pages ? (
              <Text style={styles.empty}>
                {narrowed ? "No matching sessions." : "No sessions yet."}
              </Text>
            ) : null
          }
          ListFooterComponent={
            pages?.loadingMore ? <ActivityIndicator style={styles.more} color={colors.muted} /> : null
          }
          onScrollBeginDrag={closeOpenRow}
          onEndReached={pages?.loadMore}
          onEndReachedThreshold={0.5}
          initialNumToRender={12}
          windowSize={7}
          keyboardDismissMode="interactive"
          keyboardShouldPersistTaps="handled"
          contentContainerStyle={[styles.list, { paddingTop: pageTop, paddingBottom: bottom }]}
          scrollIndicatorInsets={{ top: pageTop, bottom }}
          showsVerticalScrollIndicator={false}
        />
        <GradualBlur edge="bottom" height={bottom} color={colors.background} />
        <FileSearchBar
          value={query}
          onChange={setQuery}
          placeholder="Search sessions"
          accessory={
            <SessionFilterMenu value={filter} onChange={setFilter} harnesses={harnesses} />
          }
        />
      </View>
    </SpinnersPaused>
  );
}

/**
 * Whether a session works in `worktree`; any does without one. An older
 * desktop doesn't say where a session works, so it goes by the branch, which
 * only one working copy can have checked out.
 */
function inWorktree(session: SessionSummary, worktree: Worktree | undefined) {
  if (!worktree) return true;
  if (session.worktreePath) return samePath(session.worktreePath, worktree.path);
  return session.branch === worktree.name;
}

function samePath(a: string, b: string) {
  return a.replace(/\/+$/, "") === b.replace(/\/+$/, "");
}

/** A hairline between cards, inset to their text. */
function Divider() {
  const styles = useStyles();
  return <View style={styles.divider} />;
}

type RowHandlers = {
  open: (session: SessionSummary) => void;
  showActions: (session: SessionSummary) => void;
  rename: (session: SessionSummary) => void;
  remove: (session: SessionSummary) => void;
  swipeOpened: (row: SwipeableMethods) => void;
};

/** A session in the list, re-rendered only when it changes. */
const SessionRow = memo(function SessionRow({
  session,
  removing,
  arriving,
  handlers,
}: {
  session: SessionSummary;
  removing: boolean;
  arriving: boolean;
  handlers: RowHandlers;
}) {
  return (
    <SessionCard
      session={session}
      detailed
      onPress={() => handlers.open(session)}
      onLongPress={() => handlers.showActions(session)}
      onRename={() => handlers.rename(session)}
      onDelete={() => handlers.remove(session)}
      onSwipeOpen={handlers.swipeOpened}
      entering={arriving ? FadeIn.duration(ARRIVE_MS).delay(ARRIVE_MS * 0.3) : undefined}
      exiting={removing ? FadeOut.duration(REMOVE_MS * 0.6) : undefined}
    />
  );
});

const useStyles = themedStyles((colors) => ({
  page: {
    flex: 1,
  },
  list: {
    paddingHorizontal: 10,
  },
  empty: {
    paddingHorizontal: 14,
    paddingTop: 4,
    color: colors.muted,
    fontSize: 15,
  },
  more: {
    paddingVertical: 16,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    marginHorizontal: 14,
    backgroundColor: colors.stroke,
  },
}));
