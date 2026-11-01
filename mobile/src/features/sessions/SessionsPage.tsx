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

import { router } from "expo-router";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { FileSearchBar, SEARCH_BAR } from "@/features/files/FileSearchBar";
import { useUnreadMonos } from "@/features/workspace/monoSeen";
import type { Harness, Mono, SessionSummary, Worktree } from "@/features/workspace/types";
import { useCreateMono } from "@/features/workspace/useCreateMono";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { AnimationsPaused } from "@/ui/AnimationsPaused";
import { GradualBlur } from "@/ui/GradualBlur";
import { themedStyles, useColors } from "@/ui/theme";
import { UndoToast } from "@/ui/UndoToast";
import { useToast } from "@/ui/useToast";

import { MonoGroup } from "./MonoGroup";
import {
  finishedFromMono,
  sameOrigin,
  groupSize,
  monosForProject,
  searchMonos,
  sessionOrigin,
  type SessionOrigin,
} from "./sessionMonos";
import { setMonoGroupPrefs, useMonoGroupPrefs } from "./monoGroupPrefs";
import { SessionCard } from "./SessionCard";
import {
  filterSessions,
  NO_FILTER,
  SessionFilterMenu,
  type SessionFilter,
} from "./SessionFilterMenu";
import { useSessionLayout } from "./SessionScreen";
import { ARRIVE_MS, useArrivals } from "./useArrivals";
import { REMOVE_MS, useSessionActions } from "./useSessionActions";
import { useSessionPages, visibleSessions } from "./useSessionPages";

/**
 * A project's sessions, newest first and paged in as the list scrolls, under
 * the Monos pinned above them. Each swipes left to archive, rename or delete,
 * as on the desktop sidebar; one a Mono started says so in a chip. Searched and
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
  const { sessions, edited, rememberSession, monos, allMonos, projects } = useWorkspace();
  const desktop = useConnection().pairing?.desktop.fingerprint;
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

  const { toast, show, dismiss } = useToast();
  const sink = useMemo(() => ({ show, dismiss }), [show, dismiss]);
  const actions = useSessionActions(() => {}, sink);
  const { closeOpenRow } = actions;

  // The Monos pinned above the rows, unless the filter menu hides them.
  const prefs = useMonoGroupPrefs(desktop);
  const unread = useUnreadMonos(desktop, monos);
  const createMono = useCreateMono(projectId);
  const group = useMemo(
    () => searchMonos(monosForProject(monos, projects, projectId), needle),
    [monos, projects, projectId, needle],
  );
  // The filter offers "Monos" wherever the project has some, so it can come back.
  const hasMonos = groupSize(monosForProject(monos, projects, projectId)) > 0;
  // Who started each session, from every Mono on the desktop.
  const everyMono = useMemo(() => new Map(allMonos.map((mono) => [mono.id, mono])), [allMonos]);
  const shownMonos = useMemo(() => new Set(monos.map((mono) => mono.id)), [monos]);
  const origins = useMemo(
    () => new Map(visible.map((session) => [session.id, sessionOrigin(session, everyMono, shownMonos)])),
    [visible, everyMono, shownMonos],
  );
  const openMono = useCallback((mono: Pick<Mono, "id">) => {
    // As the drawer opens one: in place of this screen, so the drawer still
    // switches projects from it.
    router.replace({ pathname: "/mono/[monoId]", params: { monoId: mono.id } });
  }, []);
  useEffect(() => {
    if (!active) closeOpenRow();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  // The rows' handlers keep their identity, reaching the latest render's
  // through a ref, so a row only re-renders when its own session changes.
  const monoMenu = (_: SessionSummary, mono: Mono) =>
    actions.archiveFinished(finishedFromMono(visible, mono.id), mono.name);
  const latest = useRef({ onOpen, rememberSession, actions, monoMenu });
  useLayoutEffect(() => {
    latest.current = { onOpen, rememberSession, actions, monoMenu };
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
      archive: (session) => latest.current.actions.archive(session),
      openMono,
      monoMenu: (session, mono) => latest.current.monoMenu(session, mono),
      swipeOpened: (row) => latest.current.actions.swipeOpened(row),
    }),
    [openMono],
  );
  const removingId = actions.removing?.id;
  const { arriving, moving } = arrivals;
  const renderItem = useCallback(
    ({ item }: { item: SessionSummary }) => (
      <SessionRow
        session={item}
        removing={removingId === item.id}
        arriving={arriving.has(item.id)}
        origin={origins.get(item.id) ?? null}
        handlers={handlers}
      />
    ),
    [removingId, arriving, handlers, origins],
  );

  return (
    <AnimationsPaused value={!active}>
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
          extraData={[moving, origins]}
          keyExtractor={(session) => session.id}
          renderItem={renderItem}
          ItemSeparatorComponent={Divider}
          ListHeaderComponent={
            prefs.shown && groupSize(group) > 0 ? (
              <MonoGroup
                group={group}
                unread={unread}
                collapsed={prefs.collapsed}
                onToggle={() => setMonoGroupPrefs(desktop, { collapsed: !prefs.collapsed })}
                onOpen={openMono}
                onNew={createMono}
              />
            ) : null
          }
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
            <SessionFilterMenu
              value={filter}
              onChange={setFilter}
              harnesses={harnesses}
              monos={
                hasMonos
                  ? { shown: prefs.shown, onChange: (shown) => setMonoGroupPrefs(desktop, { shown }) }
                  : undefined
              }
            />
          }
        />
        <UndoToast toast={toast} onDismiss={dismiss} bottom={bottom + 8} />
      </View>
    </AnimationsPaused>
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
  archive: (session: SessionSummary) => void;
  /** A chip's Mono opens its chat. */
  openMono: (mono: Pick<Mono, "id">) => void;
  /** A chip held: archive the Mono's finished sessions. */
  monoMenu: (session: SessionSummary, mono: Mono) => void;
  swipeOpened: (row: SwipeableMethods) => void;
};

/** What a row shows of its session: a fresh list read hands over new objects
 *  for unchanged sessions, which shouldn't re-render their rows. */
function sameRow(a: SessionSummary, b: SessionSummary) {
  return (
    a === b ||
    (a.id === b.id &&
      a.title === b.title &&
      a.busy === b.busy &&
      a.updatedAt === b.updatedAt &&
      a.branch === b.branch &&
      a.monoId === b.monoId &&
      a.model === b.model &&
      a.modelName === b.modelName &&
      a.harness === b.harness)
  );
}

/** A session in the list, re-rendered only when what it shows changes. */
const SessionRow = memo(function SessionRow({
  session,
  removing,
  arriving,
  origin,
  handlers,
}: {
  session: SessionSummary;
  removing: boolean;
  arriving: boolean;
  origin: SessionOrigin | null;
  handlers: RowHandlers;
}) {
  const mono = origin?.kind === "mono" ? origin.mono : undefined;
  return (
    <SessionCard
      session={session}
      detailed
      onPress={() => handlers.open(session)}
      onLongPress={() => handlers.showActions(session)}
      onRename={() => handlers.rename(session)}
      onDelete={() => handlers.remove(session)}
      onArchive={() => handlers.archive(session)}
      origin={origin}
      onOpenOrigin={mono && origin?.kind === "mono" && origin.openable ? () => handlers.openMono(mono) : undefined}
      onOriginLongPress={mono ? () => handlers.monoMenu(session, mono) : undefined}
      onSwipeOpen={handlers.swipeOpened}
      entering={arriving ? FadeIn.duration(ARRIVE_MS).delay(ARRIVE_MS * 0.3) : undefined}
      exiting={removing ? FadeOut.duration(REMOVE_MS * 0.6) : undefined}
    />
  );
},
(before, after) =>
  before.removing === after.removing &&
  before.arriving === after.arriving &&
  before.handlers === after.handlers &&
  sameOrigin(before.origin, after.origin) &&
  sameRow(before.session, after.session));

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
