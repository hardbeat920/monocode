import { useIsFocused } from "expo-router";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  ActivityIndicator,
  FlatList,
  Keyboard,
  StyleSheet,
  View,
  type LayoutChangeEvent,
  type ScrollView,
  type ScrollViewProps,
} from "react-native";
import Animated, {
  Easing,
  FadeIn,
  useAnimatedStyle,
  useDerivedValue,
  useSharedValue,
  withTiming,
  type SharedValue,
} from "react-native-reanimated";

import type { Turn } from "@/features/workspace/types";
import {
  CollapseMotionContext,
  type CollapseMotion,
} from "@/ui/Collapsible";
import { useColors } from "@/ui/theme";
import { usePushStarted } from "@/ui/usePushedIn";

import { ChatScrollView } from "./ChatScrollView";
import { SessionErrorSheet } from "./SessionErrorSheet";
import { useJumpToEnd, useSessionLayout } from "./SessionScreen";
import type { SessionErrorNotice } from "./sessionError";
import {
  TranscriptOpened,
  TranscriptTurn,
  type TranscriptAgent,
} from "./TranscriptTurn";

const GAP = 4;
/** How far from the end, past any blank space, before the scroll-to-end button shows. */
const JUMP_DISTANCE = 160;
/** How far above the list's top inset the anchored message rests, a touch under the header. */
const ANCHOR_LIFT = 36;
/** How far the transcript rises as it fades in on open, say when switching sessions. */
const REVEAL_RISE = 10;
/** A conversation that loads within this long shows no spinner at all. */
const LOADING_AFTER = 250;
const NO_TURNS: Turn[] = [];

/**
 * Chat transcript with ChatGPT-style anchoring: the latest user message sits
 * just under the header, with blank space below it for the reply to stream into.
 * A Mono's chat, as on desktop, skips the anchoring and sits at the bottom.
 *
 * Per the keyboard-controller chat guide, that space is `blankSpace`: the viewport
 * minus everything from the anchor message down. It is a minimum inset, so the
 * keyboard opening absorbs into it instead of pushing the messages around. As the
 * reply grows the blank space shrinks to zero and the list scrolls normally.
 *
 * Opening a session is a new visit, as closing a desktop tab is: unless it is
 * still working, it opens at the end of its true height, the latest reply by
 * the composer, and anchors from the next message sent.
 *
 * It opens on the latest turns the desktop sends; scrolling up to the top
 * pages in the ones before, holding the reader's place as they land above.
 */
export function Transcript({
  turns: allTurns,
  agent,
  hasOlder = false,
  loadOlder,
  pushed = false,
  busy = false,
}: {
  turns: Turn[];
  /** Whether the session is working; one that is opens anchored. */
  busy?: boolean;
  /** On a screen pushed over another: wait for the push to start. */
  pushed?: boolean;
  agent?: TranscriptAgent;
  /** Whether there are turns before the first one here to page in. */
  hasOlder?: boolean;
  loadOlder?: () => Promise<void>;
}) {
  const colors = useColors();
  const { top, bottom } = useSessionLayout();
  // On a pushed screen the turns are drawn once the push starts, or the
  // slide waits for them to be laid out; elsewhere a frame after the list
  // mounts. The list itself mounts with the screen, as iOS blurs the native
  // header over it.
  const pushStarted = usePushStarted(pushed);
  const [drawing, setDrawing] = useState(false);
  useEffect(() => {
    if (!pushStarted) return;
    const frame = requestAnimationFrame(() => setDrawing(true));
    return () => cancelAnimationFrame(frame);
  }, [pushStarted]);
  const turns = drawing ? allTurns : NO_TURNS;
  const focused = useIsFocused();
  const [selectedError, setSelectedError] = useState<SessionErrorNotice | null>(null);
  const [errorPresented, setErrorPresented] = useState(false);
  const showError = useCallback((notice: SessionErrorNotice) => {
    Keyboard.dismiss();
    setSelectedError(notice);
    setErrorPresented(true);
  }, []);
  const list = useRef<FlatList<Turn>>(null);
  // The room under the anchored message, unclamped so a fold opening below it
  // can take from it frame by frame; the inset is what of it is left.
  const slack = useSharedValue(0);
  const blankSpace = useDerivedValue(() => Math.max(0, slack.get()));
  // While a fold moves the room is the fold's to give and take, as rows'
  // layout reaches here frames behind it; the rows settle it once it stops.
  const moving = useRef(0);
  const [, settleMotion] = useState(0);
  const motion = useMemo(
    () => ({
      begin: () => {
        moving.current += 1;
      },
      end: () => {
        moving.current -= 1;
        if (moving.current === 0) settleMotion((count) => count + 1);
      },
    }),
    [],
  );
  const heights = useRef(new Map<string, number>());
  const viewportHeight = useRef(0);
  const contentHeight = useRef(0);
  const contentInsetBottom = useRef(0);
  const anchorReady = useRef(false);
  const firstUserAnchor = useRef(false);
  // Whether the latest message holds its place under the header: decided as
  // the first turns arrive, and on with any message sent after.
  const anchoring = useRef<boolean | null>(null);
  const updateLayout = useRef<() => void>(() => {});
  // Scroll the anchor into place once its rows are measured: instantly on open,
  // animated when a new message arrives.
  const pendingScroll = useRef<"instant" | "animated" | null>("instant");
  const scrollFrame = useRef<number | null>(null);
  useEffect(() => () => {
    if (scrollFrame.current !== null) cancelAnimationFrame(scrollFrame.current);
  }, []);
  // Until that first scroll lands the list stays invisible, so it appears in
  // place rather than at its top and then jumping. Paging in history waits for
  // it too, or the list, still at its top, would ask for page after page.
  const [settled, setSettled] = useState(false);
  const [mountedAt] = useState(() => Date.now());
  const reveal = useSharedValue(0);
  useEffect(() => {
    if (settled)
      reveal.set(
        withTiming(1, { duration: 280, easing: Easing.out(Easing.cubic) }),
      );
  }, [settled, reveal]);
  const revealStyle = useAnimatedStyle(() => ({
    opacity: reveal.get(),
    transform: [{ translateY: (1 - reveal.get()) * REVEAL_RISE }],
  }));
  const [loadingOlder, setLoadingOlder] = useState(false);

  // What is there on open shows as is; replies that arrive after fade in.
  const [opened, setOpened] = useState(false);
  const hasTurns = turns.length > 0;
  useEffect(() => {
    if (!hasTurns || opened) return;
    const timer = setTimeout(() => setOpened(true), 0);
    return () => clearTimeout(timer);
  }, [hasTurns, opened]);

  const paddingTop = top;
  const paddingBottom = bottom;
  const anchorIndex = agent
    ? turns.length - 1
    : turns.findLastIndex((turn) =>
        turn.items.some((item) => item.type === "user"),
      );
  const anchorTurn = turns[anchorIndex];
  const anchorUser = anchorTurn?.items.findLast((item) => item.type === "user");
  const anchorId = agent ? anchorTurn?.id : anchorUser?.id;
  const previousAnchor = useRef({ id: anchorId, user: anchorUser, top, bottom });
  const onContentInsetChange = useCallback((insets: { bottom: number }) => {
    contentInsetBottom.current = insets.bottom;
    updateLayout.current();
  }, []);

  // Must keep its identity, or FlatList remounts every row.
  const renderScrollComponent = useCallback(
    (props: ScrollViewProps) => (
      <ChatScrollView
        {...props}
        blankSpace={blankSpace}
        onContentInsetChange={onContentInsetChange}
      />
    ),
    [blankSpace, onContentInsetChange],
  );

  const update = () => {
    anchorReady.current = false;
    if (anchoring.current === null && anchorIndex >= 0)
      anchoring.current = busy || !!anchorId?.startsWith("pending:");
    const anchored = !agent && !!anchoring.current;
    firstUserAnchor.current = anchored && anchorIndex === 0;
    if (viewportHeight.current === 0) return;
    if (anchorIndex < 0) {
      // Nothing to anchor: show what there is where it is.
      if (turns.length > 0 && !settled)
        requestAnimationFrame(() => setSettled(true));
      return;
    }
    const below = turns.slice(anchorIndex);
    let belowHeight = GAP * (below.length - 1);
    for (const turn of below) {
      const height = heights.current.get(turn.id);
      // Wait until every row from the anchor down has been measured.
      if (height === undefined) return;
      belowHeight += height;
    }
    if (moving.current === 0)
      slack.set(
        !anchored
          ? Number.NEGATIVE_INFINITY
          : viewportHeight.current -
              (paddingTop - ANCHOR_LIFT) -
              belowHeight -
              paddingBottom,
      );

    // A new conversation initially inherits the empty screen's header insets.
    // Wait for both native content layout and the bottom inset to catch up:
    // otherwise the short list clamps the first scroll to an incomplete range.
    if (firstUserAnchor.current && (
      Math.abs(contentHeight.current - (paddingTop + belowHeight + paddingBottom)) > Math.max(1, below.length) ||
      contentInsetBottom.current + 1 < blankSpace.get()
    )) return;
    anchorReady.current = true;

    if (!pendingScroll.current || scrollFrame.current !== null) return;
    // Let the new inset reach the native scroll view before scrolling to it. The
    // native scrollToEnd includes the bottom inset; FlatList's own does not. Keep
    // the request pending until it runs, so intervening layouts share one scroll.
    scrollFrame.current = requestAnimationFrame(() => {
      scrollFrame.current = requestAnimationFrame(() => {
        scrollFrame.current = null;
        if (!anchorReady.current) return;
        const mode = pendingScroll.current;
        pendingScroll.current = null;
        if (!mode) return;
        const scroll = list.current?.getNativeScrollRef() as
          ScrollView | null | undefined;
        if (firstUserAnchor.current) {
          // The first row's offset is known exactly and does not depend on the
          // keyboard or composer changing the scroll view's bottom edge.
          scroll?.scrollTo({ y: ANCHOR_LIFT, animated: mode === "animated" });
        } else {
          scroll?.scrollToEnd({ animated: mode === "animated" });
        }
        if (__DEV__ && !settled)
          console.log(
            `[mobile] transcript shown ${Date.now() - mountedAt}ms after mount`,
          );
        setSettled(true);
      });
    });
  };

  // Anchor once per user message, before layout callbacks run. The desktop
  // replaces a local pending message with a new ID; that is the same send,
  // rather than another message to scroll to. Replies and history keep its place.
  useLayoutEffect(() => {
    updateLayout.current = update;
    const previous = previousAnchor.current;
    const confirmedPending =
      !agent &&
      previous.id?.startsWith("pending:") &&
      !anchorId?.startsWith("pending:") &&
      previous.user?.text === anchorUser?.text;
    // The first load opens in place; only later messages animate in.
    if (anchorId && previous.id && anchorId !== previous.id && !confirmedPending) {
      pendingScroll.current = "animated";
      anchoring.current = true;
    } else if (!agent && anchoring.current && anchorIndex === 0 && (
      confirmedPending || top !== previous.top || bottom !== previous.bottom
    )) pendingScroll.current = "instant";
    previousAnchor.current = { id: anchorId, user: anchorUser, top, bottom };
    update();
  });

  const pageOlder = () => {
    if (!settled || !hasOlder || !loadOlder || loadingOlder) return;
    setLoadingOlder(true);
    loadOlder()
      .catch(() => {})
      .finally(() => setLoadingOlder(false));
  };

  const onRowLayout = (id: string, event: LayoutChangeEvent) => {
    const height = Math.round(event.nativeEvent.layout.height);
    if (heights.current.get(id) === height) return;
    heights.current.set(id, height);
    update();
  };

  // The composer's scroll-to-end button: this list scrolls, the frame shows it.
  const jump = useJumpToEnd();
  const jumpShown = useRef(false);
  useEffect(() => {
    if (!jump) return;
    jump.setTarget(() => {
      const scroll = list.current?.getNativeScrollRef() as
        ScrollView | null | undefined;
      scroll?.scrollToEnd({ animated: true });
    });
    return () => {
      jump.setTarget(null);
      jump.visible.set(0);
    };
  }, [jump]);
  const showJump = (show: boolean) => {
    if (!jump || jumpShown.current === show) return;
    jumpShown.current = show;
    jump.visible.set(withTiming(show ? 1 : 0, { duration: 160 }));
  };

  // A Mono's chat follows its reply down while the reader is at the end.
  const atEnd = useRef(true);
  const follow = () => {
    if (!agent || !atEnd.current || pendingScroll.current) return;
    const scroll = list.current?.getNativeScrollRef() as
      ScrollView | null | undefined;
    scroll?.scrollToEnd({ animated: true });
  };

  return (
    <TranscriptOpened value={opened}>
      {selectedError ? (
        <SessionErrorSheet
          message={selectedError.text}
          isPresented={errorPresented && focused}
          onDismiss={() => setErrorPresented(false)}
        />
      ) : null}
      <Animated.View style={[styles.fill, revealStyle]}>
        <FlatList
          ref={list}
          data={turns}
          keyExtractor={(turn) => turn.id}
          renderScrollComponent={renderScrollComponent}
          onLayout={(event) => {
            viewportHeight.current = Math.round(
              event.nativeEvent.layout.height,
            );
            update();
          }}
          contentContainerStyle={[
            styles.content,
            agent && styles.bottomAligned,
            { paddingTop, paddingBottom },
          ]}
          showsVerticalScrollIndicator={false}
          // The whole window the desktop sends, in one pass.
          initialNumToRender={25}
          onStartReached={pageOlder}
          onStartReachedThreshold={1}
          // Keep the reader's place as earlier turns land above it.
          maintainVisibleContentPosition={{ minIndexForVisible: 0 }}
          ListHeaderComponent={
            loadingOlder ? (
              <ActivityIndicator style={styles.older} color={colors.muted} />
            ) : null
          }
          onScroll={(event) => {
            const {
              contentOffset,
              contentSize,
              contentInset,
              layoutMeasurement,
            } = event.nativeEvent;
            const visibleEnd = contentOffset.y + layoutMeasurement.height;
            atEnd.current = visibleEnd >= contentSize.height - 48;
            // The bottom inset holds the blank space under an anchored message.
            showJump(
              contentSize.height + (contentInset?.bottom ?? 0) - visibleEnd >
                JUMP_DISTANCE,
            );
          }}
          scrollEventThrottle={100}
          onContentSizeChange={(_, height) => {
            contentHeight.current = height;
            update();
            follow();
          }}
          renderItem={({ item, index }) => (
            <HeldRow
              turn={item}
              height={heights.current.get(item.id) ?? 0}
              motion={motion}
              slack={!agent && index >= anchorIndex ? slack : null}
              onLayout={(event) => onRowLayout(item.id, event)}
            >
              <TranscriptTurn turn={item} agent={agent} onError={showError} />
            </HeldRow>
          )}
        />
      </Animated.View>
    </TranscriptOpened>
  );
}

/**
 * A row that keeps its height for a moment as new content shrinks it, as when
 * a live turn folds its earlier work behind the status line. The anchored
 * message's blank space grows into the difference first, so the scroll view's
 * end never comes up past the reader and the message holds its place under
 * the header. A fold the reader taps shrinks it freely: that gives its room
 * back frame by frame on its own.
 */
function HeldRow({
  turn,
  height,
  motion,
  slack,
  onLayout,
  children,
}: {
  turn: Turn;
  /** Its height as last measured, held when new content lands. */
  height: number;
  motion: Omit<CollapseMotion, "slack">;
  slack: SharedValue<number> | null;
  onLayout: (event: LayoutChangeEvent) => void;
  children: ReactNode;
}) {
  // Held from the render new content lands in, so the hold is in place in the
  // same commit as the shorter rows; at rest, nothing is held.
  const [hold, setHold] = useState({ turn, height: 0 });
  if (hold.turn !== turn) setHold({ turn, height });
  const release = useRef<number | null>(null);
  useEffect(() => () => {
    if (release.current !== null) cancelAnimationFrame(release.current);
  }, []);
  const context = useMemo(() => ({ ...motion, slack }), [motion, slack]);
  return (
    <View style={{ minHeight: hold.height }}>
      <View
        onLayout={(event) => {
          onLayout(event);
          if (hold.height === 0 || release.current !== null) return;
          // The blank space reaches the native inset on the UI thread; give
          // it a frame to land before letting the row go.
          release.current = requestAnimationFrame(() => {
            release.current = requestAnimationFrame(() => {
              release.current = null;
              setHold((current) => ({ ...current, height: 0 }));
            });
          });
        }}
      >
        <CollapseMotionContext value={context}>{children}</CollapseMotionContext>
      </View>
    </View>
  );
}

/**
 * A spinner in the middle of the transcript's space while a session's turns
 * load, so opening one shows it is on its way rather than an empty screen.
 */
export function TranscriptLoading() {
  const colors = useColors();
  const { top, bottom } = useSessionLayout();
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setShown(true), LOADING_AFTER);
    return () => clearTimeout(timer);
  }, []);
  if (!shown) return null;
  return (
    <Animated.View
      pointerEvents="none"
      entering={FadeIn.duration(200)}
      style={[styles.loading, { paddingTop: top, paddingBottom: bottom }]}
    >
      <ActivityIndicator color={colors.muted} />
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  loading: {
    position: "absolute",
    top: 0,
    right: 0,
    bottom: 0,
    left: 0,
    alignItems: "center",
    justifyContent: "center",
  },
  content: {
    gap: GAP,
  },
  fill: {
    flex: 1,
  },
  older: {
    paddingVertical: 12,
  },
  bottomAligned: {
    flexGrow: 1,
    justifyContent: "flex-end",
  },
});
